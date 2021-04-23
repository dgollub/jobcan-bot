use chrono::prelude::*;
use clap::Clap;
use color_eyre::eyre::{bail, WrapErr};
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::{thread, time};
use thirtyfour::prelude::*;
use thirtyfour::{common::command::Command, extensions::chrome::ChromeDevTools};



const TABLE_WITH_PUNSHED_DATA: usize = 6;
const COLUMN_DATE: usize = 0;
const COLUMN_HOLIDAY: usize = 1;
const COLUMN_START_TIME: usize = 2;
const COLUMN_END_TIME: usize = 3;
const COLUMN_BREAK_TIME: usize = 4;
const COLUMNS_COUNT: usize = 5;


#[allow(dead_code)]
#[derive(Deserialize)]
struct Configuration {
    #[serde(alias = "JC_LOGIN")]
    login: String,
    #[serde(alias = "JC_PASSWORD")]
    password: String,
}

impl std::fmt::Debug for Configuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Configuration")
            .field("login", &self.login)
            .field("password", &String::from("******"))
            .finish()
    }
}

/// This doc string acts as a help message when the user runs '--help'
/// as do all doc strings on fields
#[derive(Clap, Debug)]
#[clap(
    version = "1.0.0",
    author = "Daniel Kurashige-Gollub <daniel@kurashige-gollub.de>"
)]
struct Opts {
    /// Sets a custom config file. Defaults to config.toml.
    #[clap(short, long, default_value = "config.toml")]
    config: String,
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
    /// Optional memo/note for the "Push"/clock in text field. Defaults to "work"
    #[clap(short, long, default_value = "work")]
    message: String,
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
    /// Additional memo/note for the "Push"/clock in text field. Defaults to "work"
    #[clap(short, long, default_value = "work")]
    message: String,
}


/// Click on the big orange "PUSH" button.
#[derive(Clap, Debug)]
struct List {
    /// Optional date, format YYYYMM
    #[clap(short, long)]
    date: Option<String>,
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let opts: Opts = Opts::parse();

    if !Path::new(&opts.config).exists() {
        bail!("Configuration file could not be found at: {}", opts.config);
    }

    // Sanity check before we start up the browser.
    match &opts.subcmd {
        SubCommand::ReviseClockingData(revise_data) => {
            if let Some(input_date_str) = &revise_data.date {
                NaiveDate::parse_from_str(&input_date_str, "%Y-%m-%d")
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

    let config_str = fs::read_to_string(&opts.config)?;
    let config: Configuration = toml::from_str(&config_str)?;

    println!("Starting WebDriver ...");

    let mut caps = DesiredCapabilities::chrome();
    if !opts.visible {
        caps.set_headless()?;
    }

    // TODO(dkg): consider starting up Chromedriver manually here in a separate thread

    let driver = WebDriver::new("http://localhost:4444", &caps).await?;

    let dev_tools = ChromeDevTools::new(driver.session());
    let version_info = dev_tools.execute_cdp("Browser.getVersion").await?;
    println!("Using Chrome Version: {:?}", version_info);

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

    thread::sleep(time::Duration::from_millis(1000));

    // NOTE(dkg): Directly opening the edit URL or navigating there won't work and we will be prompted to login again.
    driver
        .cmd(Command::NavigateTo(String::from(
            "https://ssl.jobcan.jp/jbcoauth/login",
        )))
        .await?;

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
        }
        SubCommand::ReviseClockingData(revise_data) => {
            driver
                .cmd(Command::NavigateTo(String::from(
                    "https://ssl.jobcan.jp/employee/adit/modify/",
                )))
                .await?;

            if let Some(input_date_str) = &revise_data.date {
                let naive_date = NaiveDate::parse_from_str(&input_date_str, "%Y-%m-%d")?;
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
                    eprintln!("The format for the 'time' argument is wrong. Please check. It should be hhmm.");
                    if opts.visible && opts.sleep_time.is_none() {
                        eprintln!("Sleeping for 30 seconds. Please check the error display on the website.");
                        thread::sleep(time::Duration::from_secs(30));
                    }
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

            let tables = driver.find_elements(By::Tag("table")).await?;
            if tables.len() > TABLE_WITH_PUNSHED_DATA {
                let table = &tables[TABLE_WITH_PUNSHED_DATA];
                let body = table.find_element(By::Tag("tbody")).await?;
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
                        let _break_time = column_break_time.text().await?;

                        println!("{}: {} - {}", date, start_time, end_time);
                    }
                }
            }
        }
    }

    if let Some(sleep_time) = opts.sleep_time {
        if sleep_time > 0 {
            println!("Sleeping for {} seconds...", sleep_time);
            thread::sleep(time::Duration::from_secs(sleep_time));
        }
    }

    Ok(())
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

    // TODO(dkg): add more tests
}
