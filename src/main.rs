use chrono::prelude::*;
use clap::Clap;
use color_eyre::eyre::{bail, WrapErr};
use log::{debug, error, info, trace};
use std::{env, thread, time};
use thirtyfour::prelude::*;
use thirtyfour::{common::command::Command, extensions::chrome::ChromeDevTools};

mod config;
use crate::config::{Configuration, ENVVAR_NAME_LOGIN, ENVVAR_NAME_PASSWORD};

mod slack;
use crate::slack::post_to_slack;

const INDEX_FOR_TABLE_WITH_PUNCHED_DATA: usize = 6;
const COLUMN_DATE: usize = 0;
const COLUMN_HOLIDAY: usize = 1;
const COLUMN_START_TIME: usize = 2;
const COLUMN_END_TIME: usize = 3;
const COLUMN_BREAK_TIME: usize = 4;
const COLUMNS_COUNT: usize = 5;

const INDEX_FOR_TABLE_WITH_CURRENT_TOTALS: usize = 3;
const ROW_WITH_WORKED_HOURS_SO_FAR: usize = 0; // 1st row: 実労働時間
const ROW_WITH_WORKED_TIME_EXPECTED: usize = 1; // 2nd row: 月規定労働時間

/// This doc string acts as a help message when the user runs '--help'
/// as do all doc strings on fields
#[derive(Clap, Debug)]
#[clap(
    version = "1.1.0",
    author = "Daniel Kurashige-Gollub <daniel@kurashige-gollub.de>"
)]
struct Opts {
    /// Whether the browser window should be visible during execution. Default: not set, ie. hide the browser window.
    #[clap(short, long)]
    visible: bool,
    /// How long the program should sleep in seconds after it is done in order to keep the browser open and running.
    /// Useful for debugging together with the "visible" flag. Default: 0, meaning to not sleep and quit immediately when done.
    #[clap(short, long, name = "sleep")]
    sleep_time: Option<u64>,

    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap, Debug)]
enum SubCommand {
    #[clap(name = "push-it", alias = "clock-in", alias = "clock-out")]
    PushIt(PushIt),

    #[clap(name = "revise-clock")]
    ReviseClockingData(ReviseClockingData),

    /// Login to jobcan. Only works if the 'visible' flag is also set and 'sleep' is > 0.
    #[clap(name = "login")]
    Login,

    /// List logged hours for the current month or the given date
    #[clap(name = "list")]
    List(List),
}

/// Click on the big orange "PUSH" button.
#[derive(Clap, Debug)]
struct PushIt {
    /// Optional memo/note for the "Push"/clock in text field. Defaults to "work start"
    #[clap(short, long, default_value = "work start")]
    message: String,

    /// Message for Slack. Only used when SLACK_TOKEN and slack_channel are set.
    /// If not set no message is posted.
    #[clap(long, default_value = "", name = "slack-message")]
    slack_message: String,

    /// The Slack channel to post to. Only used when SLACK_TOKEN is set. Default: #standup
    #[clap(long, default_value = "#standup", name = "slack-channel")]
    slack_channel: String,
}

/// Add a manual time entry via the "revise clocking data" feature. Only adds new entries.
/// TODO(dkg): support removing outdated/wrong entries
#[derive(Clap, Debug)]
struct ReviseClockingData {
    /// The date that should be revised. Defaults to today. Important: format is "yyyy-MM-dd"
    #[clap(short, long)]
    date: Option<String>,
    /// The time that should be revised. Defaults to 0700, which means 7am. Important: format is "hhmm".
    #[clap(short, long, default_value = "0700")]
    time: String,
    /// Additional memo/note for the "Push"/clock in text field. Defaults to "work start"
    #[clap(short, long, default_value = "work start")]
    message: String,
}

/// Click on the big orange "PUSH" button.
#[derive(Clap, Debug)]
struct List {
    /// Optional date, format YYYYMM
    #[clap(short, long)]
    date: Option<String>,
    /// Output as CSV data. Default: false
    #[clap(short, long)]
    csv: bool,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let log_level = env::var("RUST_LOG").unwrap_or_default();
    if log_level.is_empty() {
        eprintln!("WARNING! RUST_LOG environment variable is not set. Setting it to 'info'.");
        env::set_var("RUST_LOG", "info");
    }
    color_eyre::install()?;
    dotenv::dotenv().ok();
    env_logger::init();

    let config = Configuration::from_env();
    if !config.is_ok() {
        bail!(
            "You must set both {} and {} environment variables.",
            ENVVAR_NAME_LOGIN,
            ENVVAR_NAME_PASSWORD
        );
    }

    let opts: Opts = Opts::parse();

    // Sanity check before we start up the browser.
    match &opts.subcmd {
        // Left in for testing.
        // SubCommand::PushIt(push_it) => {
        //     let _ = post_to_slack(&config, &push_it.slack_channel, &push_it.slack_message).await;
        //     return Ok(());
        // }
        SubCommand::ReviseClockingData(revise_data) => {
            if let Some(input_date_str) = &revise_data.date {
                NaiveDate::parse_from_str(input_date_str, "%Y-%m-%d")
                    .wrap_err("Unable to parse the date.")?;
            }
            if revise_data.time.len() != 4 {
                bail!("The time has a wrong format. It should be hhmm, e.g. 0700 for 7am, 2300 for 11pm, etc.");
            }
            let time = revise_data.time.parse::<i64>()?;
            // Jobcan allows apparently times greater than 2400, since 2600 is supposed to be 2am as their example
            // on their site states "ex) 2:00 a.m. ⇒　2600"
            if !(0..=2600).contains(&time) {
                bail!("The time has a wrong value. It should be between 0000 (midnight) and 2600 (2am), e.g. 0700 for 7am, 2300 for 11pm, etc.");
            }
        }
        SubCommand::Login if !opts.visible || opts.sleep_time.is_none() => {
            bail!("The 'login only' command only makes sense for debugging when the 'visible' flag set and 'sleep' is > 0.");
        }
        _ => (),
    }

    debug!("Starting WebDriver ...");

    let mut caps = DesiredCapabilities::chrome();
    if !opts.visible {
        caps.set_headless()?;
    }

    // TODO(dkg): consider starting up Chromedriver manually here in a separate thread

    let driver = WebDriver::new("http://localhost:4444", &caps).await?;

    let dev_tools = ChromeDevTools::new(driver.session());
    let version_info = dev_tools.execute_cdp("Browser.getVersion").await?;

    debug!("Using Chrome Version: {:?}", version_info);

    // Login via https://id.jobcan.jp/users/sign_in
    driver.get("https://id.jobcan.jp/users/sign_in").await?;

    let elem_form = driver.find_element(By::ClassName("form")).await?;

    // Find login input box and type in the user's login
    let elem_login = elem_form.find_element(By::Id("user_email")).await?;
    elem_login.send_keys(&config.login).await?;

    // Find password input box and type in the user's password
    let elem_password = elem_form.find_element(By::Id("user_password")).await?;
    elem_password.send_keys(&config.password).await?;

    // Click the login button
    let elem_button = elem_form.find_element(By::ClassName("form__login")).await?;
    elem_button.click().await?;

    thread::sleep(time::Duration::from_millis(1500));

    // NOTE(dkg): Directly opening the edit URL or navigating there won't work and we will be prompted to login again.
    driver
        .cmd(Command::NavigateTo(String::from(
            "https://ssl.jobcan.jp/jbcoauth/login",
        )))
        .await?;

    debug!("Waiting to avoid rate limit trigger ...");

    thread::sleep(time::Duration::from_millis(3000));

    match &opts.subcmd {
        SubCommand::PushIt(push_it) => {
            driver
                .cmd(Command::NavigateTo(String::from(
                    "https://ssl.jobcan.jp/employee",
                )))
                .await?;

            let elem_note_field = driver.find_element(By::Id("notice_value")).await?;
            elem_note_field.send_keys(&push_it.message).await?;

            let elem_push_button = driver.find_element(By::Id("adit-button-push")).await?;
            elem_push_button.click().await?;

            if config.can_post_to_slack() {
                debug!("Waiting before trying to post to Slack ...");
                thread::sleep(time::Duration::from_secs(30));

                let message = if push_it.slack_message.is_empty() {
                    &push_it.message
                } else {
                    &push_it.slack_message
                };

                let result = post_to_slack(&config, &push_it.slack_channel, message).await;
                if result.is_err() {
                    bail!("Slack returned an error.\n{}", result.unwrap_err());
                }
            }
        }
        SubCommand::ReviseClockingData(revise_data) => {
            driver
                .cmd(Command::NavigateTo(String::from(
                    "https://ssl.jobcan.jp/employee/adit/modify/",
                )))
                .await?;

            if let Some(input_date_str) = &revise_data.date {
                let naive_date = NaiveDate::parse_from_str(input_date_str, "%Y-%m-%d")?;
                driver
                    .cmd(Command::NavigateTo(format!(
                        "https://ssl.jobcan.jp/employee/adit/modify?year={}&month={}&day={}",
                        &naive_date.year(),
                        &naive_date.month(),
                        &naive_date.day()
                    )))
                    .await?;
            }

            let elem_note_time = driver.find_element(By::Id("ter_time")).await?;
            elem_note_time.send_keys(&revise_data.time).await?;

            let elem_note_field = driver
                .find_element(By::Css("textarea[name='notice']"))
                .await?;
            elem_note_field.send_keys(&revise_data.message).await?;

            let elem_insert_button = driver.find_element(By::Id("insert_button")).await?;
            elem_insert_button.click().await?;

            // Check for date or time errors
            let elem_time_error = driver.find_element(By::Id("time_error")).await;
            if let Ok(elem) = elem_time_error {
                let elem_error = elem.find_element(By::ClassName("alert")).await;
                if elem_error.is_ok() {
                    error!("The format for the 'time' argument is wrong. Please check. It should be 'hhmm'.");
                    if opts.visible && opts.sleep_time.is_none() {
                        error!("Sleeping for 90 seconds. Please check the error display on the website.");
                        thread::sleep(time::Duration::from_secs(90));
                    }
                    bail!("The 'time' argument has the wrong format. It should be 'hhmm'.");
                }
            }
        }
        SubCommand::Login => {
            driver
                .cmd(Command::NavigateTo(String::from(
                    "https://ssl.jobcan.jp/employee/adit/modify/",
                )))
                .await?;
        }
        SubCommand::List(list) => {
            driver
                .cmd(Command::NavigateTo(String::from(
                    "https://ssl.jobcan.jp/employee/attendance",
                )))
                .await?;

            thread::sleep(time::Duration::from_millis(500));

            debug!("Checking if we were redirected to the partial error page ...");

            let right_url = driver.current_url().await?;
            if right_url.contains("error/partial-rate-limit") {
                driver.back().await?;
                thread::sleep(time::Duration::from_millis(500));
            }

            if let Some(input_date_str) = &list.date {
                let full_input_date = format!("{}01", input_date_str); // format is YYYYMM
                let naive_date = NaiveDate::parse_from_str(&full_input_date, "%Y%m%d")?;

                driver
                    .cmd(Command::NavigateTo(format!(
                        "https://ssl.jobcan.jp/employee/attendance?list_type=normal&search_type=month&year={}&month={}",
                        &naive_date.year(),
                        &naive_date.month()
                    )))
                    .await?;
            }

            if !list.csv {
                let title_element = driver.find_element(By::ClassName("card-title")).await;
                if let Ok(title) = title_element {
                    info!("---------------------------");
                    info!("Data for {}", title.text().await?);
                    info!("---------------------------");
                }
            }

            let tables = driver.find_elements(By::Tag("table")).await?;
            if tables.len() > INDEX_FOR_TABLE_WITH_PUNCHED_DATA {
                let table = &tables[INDEX_FOR_TABLE_WITH_PUNCHED_DATA];
                let body = table.find_element(By::Tag("tbody")).await?;
                let mut total_punched_minutes: u32 = 0;
                let mut total_break_minutes: u32 = 0;

                for tr in body.find_elements(By::Tag("tr")).await? {
                    let columns = tr.find_elements(By::Tag("td")).await?;
                    if columns.len() >= COLUMNS_COUNT {
                        let column_date = &columns[COLUMN_DATE];
                        let column_holiday = &columns[COLUMN_HOLIDAY];
                        let column_start_time = &columns[COLUMN_START_TIME];
                        let column_end_time = &columns[COLUMN_END_TIME];
                        let column_break_time = &columns[COLUMN_BREAK_TIME];

                        let date = column_date.text().await?;
                        let _holiday = column_holiday.text().await?;
                        let start_time = column_start_time.text().await?;
                        let end_time = column_end_time.text().await?;
                        let break_time = column_break_time.text().await?;

                        if !list.csv {
                            info!(
                                "{}: {} - {} (break: {})",
                                date, start_time, end_time, break_time
                            );
                        }

                        if !start_time.is_empty() {
                            let start = calc_minutes(&start_time);
                            let end = calc_minutes(&end_time);
                            if start.is_none() || end.is_none() {
                                if !list.csv {
                                    debug!("<--- previous ignored, either start or end is 0");
                                }
                                continue;
                            }
                            let break_minutes = calc_minutes(&break_time).unwrap_or_default();
                            let total_for_day = end.unwrap() - start.unwrap();

                            total_punched_minutes += total_for_day;
                            total_break_minutes += break_minutes;

                            if list.csv {
                                // NOTE(dkg): With default language being Japanese, the output means the following
                                // mm/dd, hh:mm (start); hh:mm (end), hh:mm (break duration), minutes (total work time without breaks)
                                let total_for_day_without_breaks = total_for_day - break_minutes;
                                let hours = total_for_day_without_breaks / 60;
                                let minutes = total_for_day_without_breaks % 60;
                                println!(
                                    "{};{};{};{};{:02}:{:02}",
                                    date, start_time, end_time, break_time, hours, minutes
                                );
                            }
                        }
                    }
                }

                let jobcan_calculated_data = if tables.len() > INDEX_FOR_TABLE_WITH_CURRENT_TOTALS {
                    let table = &tables[INDEX_FOR_TABLE_WITH_CURRENT_TOTALS];
                    let body = table.find_element(By::Tag("tbody")).await?;
                    let rows = body.find_elements(By::Tag("tr")).await?;

                    if rows.len() > ROW_WITH_WORKED_TIME_EXPECTED {
                        let row_worked_so_far = &rows[ROW_WITH_WORKED_HOURS_SO_FAR];
                        let row_worked_expected = &rows[ROW_WITH_WORKED_TIME_EXPECTED];

                        let col_worked_so_far =
                            row_worked_so_far.find_element(By::Tag("td")).await?;
                        let col_worked_expected =
                            row_worked_expected.find_element(By::Tag("td")).await?;

                        let worked_so_far = col_worked_so_far.text().await?;
                        let worked_expected = col_worked_expected.text().await?;

                        if !list.csv {
                            info!("------------ Jobcan says ---------------");
                            info!("Worked  : {}", worked_so_far);
                            info!("Expected: {}", worked_expected);
                            info!("----------------------------------------");
                        }

                        Some((worked_expected, worked_so_far))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let (punched_hours, punched_minutes) = if total_punched_minutes > 0 {
                    let hours_worked = total_punched_minutes / 60;
                    let minutes_worked = total_punched_minutes % 60;
                    let hours_break = total_break_minutes / 60;
                    let minutes_break = total_break_minutes % 60;
                    let total_punched_minutes_without_breaks =
                        total_punched_minutes - total_break_minutes;
                    let hours_worked_no_breaks = total_punched_minutes_without_breaks / 60;
                    let minutes_worked_no_breaks = total_punched_minutes_without_breaks % 60;

                    if !list.csv {
                        info!(
                            "\nTotal amount of time worked: {} minutes, or {:02}:{:02} hh:mm (breaks: {:02}:{:02})",
                            total_punched_minutes, hours_worked, minutes_worked, hours_break, minutes_break,
                        );
                        info!("Total amount of time worked (ignoring breaks): {} minutes, or {:02}:{:02} hh:mm",
                            total_punched_minutes_without_breaks, hours_worked_no_breaks, minutes_worked_no_breaks,
                        );
                    }

                    (hours_worked_no_breaks, minutes_worked_no_breaks)
                } else {
                    (0, 0)
                };

                if let Some((expected, so_far)) = jobcan_calculated_data {
                    if !list.csv {
                        info!("---------------------------");
                        info!("required {} and {}", expected, so_far);
                        info!(
                            "punched  {}:{} and {}:{}",
                            punched_hours, punched_minutes, punched_hours, punched_minutes
                        );
                        info!("---------------------------");
                    }
                }
            }
        }
    }

    if let Some(sleep_time) = opts.sleep_time {
        if sleep_time > 0 {
            trace!("Sleeping for {} seconds...", sleep_time);
            thread::sleep(time::Duration::from_secs(sleep_time));
        }
    }

    Ok(())
}

/// Turn timestamps like 06:45 into total minutes from 00:00 onwards.
/// Example: 06:45 would be 6 * 60 + 45 = 360 + 45 = 405 minutes
fn calc_minutes(time_string: &str) -> Option<u32> {
    if time_string.is_empty() {
        return None;
    }
    if !time_string.contains(':') {
        return None;
    }
    if time_string.len() < 2 {
        return None;
    }
    let index = 2;
    let (front, back) = time_string.split_at(index);
    let hours = front[..index].parse::<u32>().ok()?;
    let minutes = back[1..].parse::<u32>().ok()?;

    Some(hours * 60 + minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date_input() {
        let input_date_str = String::from("2020-01-31");
        let date_only = NaiveDate::parse_from_str(&input_date_str, "%Y-%m-%d");
        assert_eq!(
            date_only.unwrap().format("%Y-%m-%d").to_string(),
            input_date_str
        );
    }

    #[test]
    fn test_calc_minutes_works_1() {
        let input = String::from("09:51");
        let minutes = calc_minutes(&input).unwrap_or_default();

        assert_eq!(9 * 60 + 51, minutes);
    }

    #[test]
    fn test_calc_minutes_works_2() {
        let input = String::from("23:59");
        let minutes = calc_minutes(&input).unwrap_or_default();

        assert_eq!(23 * 60 + 59, minutes);
    }

    #[test]
    fn test_calc_minutes_works_3() {
        let input = String::from("00:01");
        let minutes = calc_minutes(&input).unwrap_or_default();

        assert_eq!(1, minutes);
    }

    #[test]
    fn test_calc_minutes_returns_0_on_failure_1() {
        let input = String::from("勤務中");
        let minutes = calc_minutes(&input);

        assert_eq!(None, minutes);
    }

    #[test]
    fn test_calc_minutes_returns_0_on_failure_2() {
        let input = String::from("11:mm");
        let minutes = calc_minutes(&input);

        assert_eq!(None, minutes);
    }

    #[test]
    fn test_calc_minutes_returns_0_on_failure_3() {
        let input = String::from("mm:11");
        let minutes = calc_minutes(&input);

        assert_eq!(None, minutes);
    }

    #[test]
    fn test_calc_minutes_returns_0_on_failure_4() {
        let input = String::from(":");
        let minutes = calc_minutes(&input);

        assert_eq!(None, minutes);
    }

    #[test]
    fn test_calc_minutes_returns_0_on_failure_5() {
        let input = String::from("0:0");
        let minutes = calc_minutes(&input);

        assert_eq!(None, minutes);
    }

    // TODO(dkg): add more tests
}
