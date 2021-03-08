use crate::{Server_Event, TextMessage, DatabaseUser, V1Client, RepeatedField, future, FutureValue};
use regex::Regex;
use std::sync::{Arc, Mutex};

const KEYWORD: &'static str = "!rename ";
const NAME_REGEX: &'static str = r"^[\w@\{\}\(\)\|\._-]{1,25}$";
const INVALID_NICKNAME_MESSAGE: &'static str = "<b><span style=\"color:#aa0000\">Invalid Nickname</span></b>";

pub fn change_name<D>(_t: Arc<Mutex<D>>, c: V1Client, e: Server_Event) -> FutureValue<bool> {
    future(async move {
        if !e.has_user() || !e.has_message() || !e.get_user().has_id() {
            return true;
        }
        let user = e.get_user();
        let id = user.get_id();
        let message = e.get_message();
        let text = message.get_text().to_owned();
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
                let mut database_user = DatabaseUser::new();
                database_user.set_server(e.get_server().to_owned());
                database_user.set_id(id);
                if let Ok(u) = c.database_user_get_async(&database_user) {
                    if let Ok(mut database_user) = u.await {
                        database_user.set_name(new_name);
                        database_user.set_id(id);
                        if let Ok(u) = c.database_user_update_async(&database_user) { drop(u.await); }
                    } 
                }
            } else {
                let mut error_message = TextMessage::new();
                error_message.set_server(e.get_server().to_owned());
                error_message.set_users(RepeatedField::from_vec(vec![user.to_owned()]));
                error_message.set_text(INVALID_NICKNAME_MESSAGE.to_owned());
                if let Ok(u) = c.text_message_send_async(&error_message) { drop(u.await); }
            }
            return false;
        }
        true
    })
}
