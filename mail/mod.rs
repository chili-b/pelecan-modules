use crate::{server::Event, DataMutex, Client, FutureBool, future_from_async, Persistent, future_from_bool, Server, User, TextMessage, DatabaseUser};
use crate::database_user::Query;
use super::Data;
use confy::ConfyError;
use serde_derive::{Serialize, Deserialize};

const MAX_MESSAGES: usize = 5;

pub fn text_message(t: DataMutex<Data>, mut c: Client, e: &Event) -> FutureBool {
    let e = e.to_owned();
    if e.message.is_none() || e.server.is_none() || e.user.is_none() || e.user.as_ref().unwrap().id.is_none() {
        return future_from_bool(true);
    }
    let text = e.message.unwrap().text().to_owned();
    let user = e.user.unwrap();
    if text.starts_with("!mail") {
        let words: Vec<&str> = text.split(" ").collect();
        if words.len() < 2 {
            let server = e.server.clone();
            return future_from_async(async move {
                drop(c.text_message_send(TextMessage {
                    server: server,
                    users: vec![user],
                    text: Some("<br/>!mail commands:<ul> \
                               <li>send [recipient] [message] (Send a message to the user with the given name.)</li> \
                               <li>read [number] (Read a message. Typing <tt>!mail read</tt> without a number will open your mailbox.)</li> \
                               <li>delete [number] (Delete a message. Typing <tt>!mail delete</tt> without a number will empty your mailbox.)</li></ul>".to_string()),
                    channels: vec![], trees: vec![], actor: None
                }).await);
                false
            })
        }
        return match words[1] {
            "send" => send_message(c, t, e.server, user, text[10..].to_owned()),
            "read" => read_message(c, t, e.server, user, text[10..].to_owned()),
            "delete" => delete_message(c, t, e.server, user, text[12..].to_owned()),
            _ => future_from_bool(true)
        };
    }
    future_from_bool(true)
}

pub fn user_connected(mut t: DataMutex<Data>, mut c: Client, e: &Event) -> FutureBool {
    let e = e.to_owned();
    if e.user.is_none() || e.user.as_ref().unwrap().id.is_none() {
        return future_from_bool(true);
    }
    let user = e.user.unwrap();
    let server = e.server.clone();
    future_from_async(async move { 
        let server_name = &t.lock_async().await.name;
        let mailbox = load_mailbox(user.id(), server_name);
        if !mailbox.messages.is_empty() {
            drop(c.text_message_send(TextMessage {
                server: server.clone(),
                users: vec![user],
                text: Some(mailbox_string(c.clone(), server, mailbox).await),
                channels: vec![], trees: vec![], actor: None,
            }).await);
        }
        true
    })
}

fn delete_message(mut c: Client, mut t: DataMutex<Data>, server: Option<Server>, user: User, message: String) -> FutureBool {
    let message = message.trim().to_owned();
    future_from_async(async move {
        let server_name = &t.lock_async().await.name;
        let mut mailbox = load_mailbox(user.id(), server_name);
        if message.len() == 0 {
            mailbox.messages = vec![];
            drop(store_mailbox(&mailbox, server_name));
            drop(c.text_message_send(TextMessage {
                server: server,
                users: vec![user],
                text: Some("Emptied mailbox".to_string()),
                channels: vec![], trees: vec![], actor: None
            }).await);
            false
        } else if let Ok(index) = message.parse::<usize>() {
            if mailbox.remove_message(index) {
                drop(store_mailbox(&mailbox, server_name));
                drop(c.text_message_send(TextMessage {
                    server: server,
                    users: vec![user],
                    text: Some("Deleted 1 message".to_string()),
                    channels: vec![], trees: vec![], actor: None
                }).await);
            }
            false
        } else {
            true
        }
    })
}
fn send_message(mut c: Client, mut t: DataMutex<Data>, server: Option<Server>, user: User, message: String) -> FutureBool {
    let message = message.trim_start();
    if let Some(index) = message.find(" ") {
        let (recipient, message) = message.trim_start().split_at(index);
        let recipient = recipient.to_owned();
        let message = message.trim().to_owned();
        return future_from_async(async move {
            let server_name = t.lock_async().await.name.to_owned();
            if let Some(recipient_id) = user_id_from_name(c.clone(), server.clone(), &recipient).await {
                let mut mailbox = load_mailbox(recipient_id, &server_name);
                mailbox.owner = recipient_id; // if the mailbox is newly created, it will have the wrong owner id.
                let message = Message::new(user.id(), message.to_string());
                if mailbox.add_message(&server_name, message) {
                    let success_message = TextMessage {
                        server: server,
                        users: vec![user],
                        text: Some(format!(
                                "Message sent to <b><span style=\"color:#55aa7f\">{}</span></b>'s mailbox", 
                                recipient)),
                        channels: vec![], trees: vec![], actor: None
                    };
                    drop(store_mailbox(&mailbox, &server_name));
                    drop(c.text_message_send(success_message).await);
                } else {
                    let error_message = TextMessage {
                        server: server,
                        users: vec![user],
                        text: Some(format!(
                                "<b><span style=\"color:#55aa7f\">{}</span></b>'s mailbox is full so your message was not delivered", 
                                recipient)),
                        channels: vec![], trees: vec![], actor: None
                    };
                    drop(c.text_message_send(error_message).await);
                }
            } else {
                let error_message = TextMessage {
                    server: server,
                    users: vec![user],
                    text: Some(format!(
                            "<b><span style=\"color:#aa0000\">Couldn't find a user with the name `{}`</span></b>",
                            recipient)),
                    channels: vec![], trees: vec![], actor: None
                };
                drop(c.text_message_send(error_message).await);
            }
            false
        });
    }
    future_from_bool(false)
}

fn read_message(mut c: Client, mut t: DataMutex<Data>, server: Option<Server>, user: User, message: String) -> FutureBool {
    let message = message.trim().to_owned();
    future_from_async(async move {
        let server_name = &t.lock_async().await.name;
        let mut mailbox = load_mailbox(user.id(), server_name);
        mailbox.owner = user.id(); // if the mailbox is newly created, it will have the wrong id.
        if let Ok(index) = message.parse::<usize>() {
            if let Some(message) = mailbox.read_message(server_name, index) {
                let text_message = TextMessage {
                    server: server.clone(),
                    users: vec![user],
                    text: message_string(c.clone(), server, message).await,
                    channels: vec![], trees: vec![], actor: None
                };
                drop(c.text_message_send(text_message).await);
                drop(store_mailbox(&mailbox, server_name));
            }
        } else {
            let mailbox_message = TextMessage {
                server: server.clone(),
                users: vec![user],
                text: Some(mailbox_string(c.clone(), server, mailbox).await),
                channels: vec![], trees: vec![], actor: None
            };
            drop(c.text_message_send(mailbox_message).await);
        }
        false
    })
}

async fn user_id_from_name(mut c: Client, server: Option<Server>, name: &str) -> Option<u32> {
    let query = Query {server: server, filter: Some(name.to_string())};
    if let Ok(response) = c.database_user_query(query).await {
        let list = response.into_inner();
        if list.users.len() > 0 {
            return Some(list.users[0].id());
        }
    }
    None
}

async fn user_name_from_id(mut c: Client, server: Option<Server>, id: u32) -> Option<String> {
    let database_user = DatabaseUser {
        server: server, id: Some(id), name: None,
        email: None, comment: None, hash: None,
        password: None, last_active: None, texture: None};
    if let Ok(response) = c.database_user_get(database_user).await {
        let database_user = response.into_inner();
        return database_user.name;
    }
    None
}

async fn message_string(c: Client, server: Option<Server>, message: Message) -> Option<String> {
    if let Some(name) = user_name_from_id(c, server, message.sender).await {
        return Some(format!("<h3>From: {}</h3>{}", name, message.contents));
    }
    None
}

async fn mailbox_string(c: Client, server: Option<Server>, mailbox: Mailbox) -> String {
    let mut mailbox_string = "<tt><h3>Your Mailbox:</h3>".to_string();
    for (index, message) in mailbox.messages.iter().enumerate() {
        let sender = if let Some(name) = user_name_from_id(c.clone(), server.clone(), message.1).await {
            name
        } else {
            "The user with this ID is no longer registered".to_string()
        };
        let mut mailbox_entry = format!("[{}] From: {}</span></b><br/>", index, sender);
        if message.0 { // if the message is unread, bold and highlight it
            mailbox_entry = format!("<b><span style=\"color:#5555ff\">{}", mailbox_entry);
        }
        mailbox_string.push_str(&mailbox_entry);
    }
    mailbox_string.push_str("<sup>To read a specific message, type <b>!mail read [number]</b></sup></tt>");
    mailbox_string
}

fn load_mailbox(owner: u32, server_name: &str) -> Mailbox {
    Mailbox::load(&mailbox_name(server_name, owner))
}

fn store_mailbox(mailbox: &Mailbox, server_name: &str) -> Result<(), ConfyError> {
    mailbox.store(&mailbox_name(server_name, mailbox.owner))
}

fn message_name(server_name: &str, owner: u32, num: usize) -> String {
    format!("pelecan_message_{}_{}_{}", server_name, owner, num)
}

fn mailbox_name(server_name: &str, owner: u32) -> String {
    format!("pelecan_mailbox_{}_{}", server_name, owner)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Mailbox {
    owner: u32,
    messages: Vec<(bool, u32, String)>
}

impl Persistent for Mailbox {}

impl std::default::Default for Mailbox {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Mailbox {
    pub fn new(owner: u32) -> Self {
        Self {
            owner: owner,
            messages: vec![]
        }
    }

    pub fn add_message(&mut self, server_name: &str, message: Message) -> bool {
        if self.messages.len() > MAX_MESSAGES {
            return false;
        }
        let message_name = message_name(server_name, self.owner, self.messages.len());
        self.messages.push((true, message.sender, message_name.clone()));
        drop(message.store(&message_name));
        true
    }

    pub fn remove_message(&mut self, index: usize) -> bool {
        if index >= self.messages.len() {
            return false;
        }
        self.messages.remove(index);
        true
    }

    pub fn read_message(&mut self, server_name: &str, index: usize) -> Option<Message> {
        if let Some(message) = self.messages.get_mut(index) {
            message.0 = false;
            return Some(Message::load(&message_name(server_name, self.owner, index)));
        }
        None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    sender: u32,
    is_unread: bool,
    contents: String,
}

impl Persistent for Message {}

impl std::default::Default for Message {
    fn default() -> Self {
        Self::new(0, "".to_owned())
    }
}

impl Message {
    pub fn new(sender: u32, contents: String) -> Self {
        Self {
            sender: sender,
            is_unread: true,
            contents: contents
        }
    }
}
