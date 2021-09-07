# Jobcan Bot for automatically filling out time tracking on jobcan.ne.jp

This bot allows you to fill out your timesheet in [Jobcan](https://jobcan.ne.jp/) automatically.
It utilizes WebDriver/Selenium for this task.

## How to run

You will need to have the [Rust programming language](https://www.rust-lang.org/) installed (including Cargo).

You will need to manually start [ChromeDriver](https://chromedriver.chromium.org/) via `chromedriver --port=4444` in a different terminal window or tab.
Then simply run `cargo run -- --config=<path-to-your-configuration-file>`. You can install chromedriver on macOS
via `brew install chromedriver`.

You will need to either have a CSV file with your timesheet data or manually input the data for the specified date.
See `cargo run -- --help` for available options.

## Note

You must have logged into Jobcan's website manually at least once before in order to set your password.

## Configuration

The configuration is done in a `.env` file. It is expected to be in the same folder as the executable.
See [.env.example](.env.example) for the available configuration options.

### Slack integration

You can also configure this bot to automatically post a message to a specific Slack channel after punshing into Jobcan.
You will need to set the `SLACK_USER_TOKEN` environment variable (or set it in your `.env` file). Then you can specify the
channel and message via the `--slack-channel` (default: `#standup`) and `--slack-message` command line arguments.

By default `--slack-message` will use the same message you specified for Jobcan.

## Copyright

Copyright ©️ 2021 by Daniel Kurashige-Gollub <daniel@kurashige-gollub.de>

## License

MIT
