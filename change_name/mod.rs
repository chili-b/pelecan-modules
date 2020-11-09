use crate::{server::Event, TextMessage, DataMutex, DatabaseUser, 
            Client, FutureBool, future_from_async};
use regex::Regex;

const KEYWORD: &'static str = "!nickname ";
const NAME_REGEX: &'static str = r"^[\w@\{\}\(\)\|\._-]{1,25}$";
const INVALID_NICKNAME_MESSAGE: &'static str = "<b><span style=\"color:#aa0000\">Invalid Nickname!</span></b>";

pub fn change_name<Data>(_t: DataMutex<Data>, mut c: Client, e: &Event) -> FutureBool {
    // Get an owned value of e so it can be used inside the async block.
    let e = e.to_owned();
    future_from_async(async move {
        // Ensure event contains correct data.
        //
        // Only allow registered users to use this 
        // command since unregistered users can already
        // disconnect and reconnect with a new name.
        if e.user.is_none() || e.message.is_none() || e.user.as_ref().unwrap().id.is_none() {
            return true;
        }
        let user = e.user.unwrap();
        let id = user.id.unwrap();
        let message = e.message.unwrap();
        let text = message.text().to_owned();
        let regex = if let Ok(regex) = Regex::new(NAME_REGEX) {
            regex
        } else {
            eprintln!("Invalid regex set in change_name module.");
            return true;
        };
        if text.starts_with(KEYWORD) {
            let new_name = text[KEYWORD.len()..].to_owned();
            if regex.is_match(&new_name) {
                // Update the database
                let database_user = DatabaseUser {
                    server: e.server.clone(),
                    id: Some(id),
                    name: None, email: None, comment: None, hash: None,
                    password: None, last_active: None, texture: None
                };
                if let Ok(response) = c.database_user_get(database_user).await {
                    let mut database_user = response.into_inner();
                    database_user.name = Some(new_name);
                    database_user.id = Some(id);
                    if let Err(status) = c.database_user_update(database_user).await {
                        let error_message = TextMessage {
                            server: e.server,
                            users: vec![user],
                            text: Some(status.message().to_owned()),
                            actor: None, channels: vec![], trees: vec![]
                        };
                        drop(c.text_message_send(error_message).await);
                    }
                } 
            } else {
                let error_message = TextMessage {
                    server: e.server,
                    users: vec![user],
                    text: Some(INVALID_NICKNAME_MESSAGE.to_owned()),
                    actor: None, channels: vec![], trees: vec![]
                };
                drop(c.text_message_send(error_message).await);
            }
        }
        // If the user is changing their nickname, no further parsing should be done on
        // the message regardless of whether or not the name was valid.
        false
    })
}
