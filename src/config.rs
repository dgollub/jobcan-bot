use std::env;

pub const ENVVAR_NAME_LOGIN: &str = "JC_LOGIN";
pub const ENVVAR_NAME_PASSWORD: &str = "JC_PASSWORD";
pub const ENVVAR_SLACK_USER_TOKEN: &str = "SLACK_USER_TOKEN";
pub const ENVVAR_SLACK_USER_NAME: &str = "SLACK_USER_NAME";

#[derive(Default)]
pub struct Configuration {
    pub login: String,
    pub password: String,
    pub slack_user_token: String,
    pub slack_user_name: String,
}

impl std::fmt::Debug for Configuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Configuration")
            .field("login", &self.login)
            .field("password", &String::from("******"))
            .finish()
    }
}

impl Configuration {
    pub fn from_env() -> Self {
        let login = env::var(ENVVAR_NAME_LOGIN).unwrap_or_default();
        let password = env::var(ENVVAR_NAME_PASSWORD).unwrap_or_default();
        let slack_user_token = env::var(ENVVAR_SLACK_USER_TOKEN).unwrap_or_default();
        let slack_user_name = env::var(ENVVAR_SLACK_USER_NAME).unwrap_or_default();

        Configuration {
            login,
            password,
            slack_user_token,
            slack_user_name,
        }
    }

    pub fn is_ok(&self) -> bool {
        !self.login.is_empty() && !self.password.is_empty()
    }

    pub fn can_post_to_slack(&self) -> bool {
        !self.slack_user_token.is_empty() && !self.slack_user_name.is_empty()
    }
}
