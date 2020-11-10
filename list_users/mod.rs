use crate::{FutureBool, Client, TextMessage, server::Event, DataMutex, future_from_async, future_from_bool,};
use crate::database_user::Query;

pub fn list_users<D>(_t: DataMutex<D>, mut c: Client, e: &Event) -> FutureBool {
    let e = e.to_owned();
    if e.server.is_none() || e.user.is_none() || e.message.is_none() {
        return future_from_bool(true);
    }
    let user = e.user.unwrap();
    let server = e.server;
    let message = e.message.as_ref().unwrap().text().to_owned();
    if message.trim() == "!users" {
        return future_from_async(async move {
            let query = Query {
                server: server.clone(),
                filter: None,
            };
            if let Ok(response) = c.database_user_query(query).await {
                let user_list = response.into_inner().users.into_iter().fold("<b>Registered usernames:</b><br/>".to_string(), |mut list, user| {
                    list.push_str(&format!("{}<br/>", user.name()));
                    list
                });
                let user_list_message = TextMessage {
                    server: server,
                    actor: None,
                    users: vec![user],
                    channels: vec![],
                    trees: vec![],
                    text: Some(user_list)
                };
                drop(c.text_message_send(user_list_message).await);
            }
            false
        });
    }
    future_from_bool(true)
}
