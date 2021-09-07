use log::{debug, warn};
use slack_morphism::prelude::*;

use crate::config::{Configuration, ENVVAR_SLACK_TOKEN, ENVVAR_SLACK_USER_NAME};

pub async fn post_to_slack(
    config: &Configuration,
    channel: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !config.can_post_to_slack() {
        debug!(
            "'{}' and '{}' environment variable must be set in order to post to Slack -> ignoring",
            ENVVAR_SLACK_TOKEN, ENVVAR_SLACK_USER_NAME,
        );
        return Ok(());
    }

    let username = &config.slack_user_name;

    if !channel.contains('#') {
        // TODO(dkg): improve error handling
        panic!("The Slack channel name must contain the leading '#'.");
    }

    debug!(
        "Posting message to Slack channel '{}' as user '{}'.",
        channel, username
    );

    use slack_morphism::*;
    // Slack Morphism Hyper/Tokio support
    use slack_morphism_hyper::*;

    let hyper_connector = SlackClientHyperConnector::new();
    let client = SlackClient::new(hyper_connector);

    let token_value = config.slack_token.clone();
    let token_value: SlackApiTokenValue = token_value.into();
    let token: SlackApiToken = SlackApiToken::new(token_value);

    // Create a Slack session with this token
    let session = client.open_session(&token);

    let user_list_req = SlackApiUsersListRequest::new();
    let user_list_res = session.users_list(&user_list_req).await?;
    // eprintln!("{:#?}", user_list_res.members);
    let search_for_user = Some(username.into());
    let slack_user = user_list_res
        .members
        .into_iter()
        .find(|user| user.name.eq(&search_for_user));
    // TODO(dkg): improve error handling
    let slack_user = slack_user.unwrap_or_else(|| {
        panic!(
            "The Slack user '{}' could not be found in the workspace.",
            &username
        )
    });
    let user_info_req = SlackApiUsersInfoRequest::new(SlackUserId(slack_user.id.to_string()));

    let user_info_resp = session.users_info(&user_info_req).await?;
    let slack_user = user_info_resp.user;
    eprintln!("{:#?}", slack_user);

    // Send a simple text message
    let mut post_chat_req = SlackApiChatPostMessageRequest::new(
        channel.into(),
        SlackMessageContent::new().with_text(message.into()),
    );
    if let Some(profile) = slack_user.profile {
        post_chat_req.username(profile.display_name.unwrap_or_else(|| username.into()));
        if let Some(icon) = profile.icon {
            if let Some(images) = icon.images {
                // TODO(dkg): not sure if this is the right one to use...
                let resolution48 = images.resolutions.into_iter().find(|(r, _)| *r == 48);
                if let Some(resolution48) = resolution48 {
                    post_chat_req.icon_url(resolution48.1);
                } else {
                    warn!("Profile icon with size 48x48 not found.");
                }
            } else {
                eprintln!("No image_original");
            }
        } else {
            eprintln!("No profile icon");
        }
    } else {
        panic!("The user '{}' has no user profile on Slack.", username);
    }

    let post_chat_resp = session.chat_post_message(&post_chat_req).await?;
    eprintln!("response: {:#?}", post_chat_resp);
    Ok(())
}
