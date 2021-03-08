use crate::{Server_Event, V1Client, FutureValue, future, Server, User, TextMessage, DatabaseUser, DatabaseUser_Query, TextMessage_Filter, TextMessage_Filter_Action};
use super::Data;
use confy::ConfyError;
use serde_derive::{Serialize, Deserialize};
use std::path::PathBuf;
use rand::Rng;
use std::sync::{Arc, Mutex};

const MAX_MESSAGES: usize = 5;

pub fn chat_filter<T>(t: Arc<Mutex<Data>>, mut c: V1Client, filter: mut TextMessage_Filter) -> FutureValue<(bool, TextMessage_Filter)> {
    future(async move {
        if filter.message.is_none() || filter.server.is_none() {
            return (true, filter);
        }
        let text = filter.message.as_ref().unwrap().text().to_owned();
        if filter.message.as_ref().unwrap().actor.is_none() {
            return (true, filter);
        }
        let mut user = filter.message.as_ref().unwrap().actor.as_ref().unwrap().to_owned();
        if text.starts_with("!mail") {
            filter.set_action(Action::Drop);
            let filter = filter.to_owned();
            return future_from_async(async move {
                // for whatever reason, user id is not available to the filter stream, so we have to do
                // a database query... very stupid, but that's the way it is.
                user = if let Ok(user) = c.user_get(user).await {
                    user.into_inner()
                } else {
                    return false;
                };
                if user.id.is_none() {
                    return true;
                }
                let words: Vec<&str> = text.split(" ").collect();
                if words.len() < 2 {
                    let server = filter.server.clone();
                    drop(c.text_message_send(TextMessage {
                        server: server,
                        users: vec![user],
                        text: Some("<br/><h3>📨 !mail commands:</h3><ul> \
                                   <li><tt>!mail send <b>recipient</b> <b>message</b></tt><br/>(Send a <b>message</b> to the user with the given name.)</li> \
                                <li><tt>!mail read <b>number</b></tt><br/>(Read a message. Typing <tt>!mail read</tt> without a number will open your mailbox.)</li> \
                                <li><tt>!mail delete <b>number</b></tt><br/>(Delete a message. Typing <tt>!mail delete</tt> without a number will empty your mailbox.)</li></ul>".to_string()),
                                channels: vec![], trees: vec![], actor: None
                    }).await);
                    false
                } else {
                    match words[1] {
                        "send" => send_message(c, t, filter.server.to_owned(), user, text[10..].to_owned()).await,
                        "read" => read_message(c, t, filter.server.to_owned(), user, text[10..].to_owned()).await,
                        "delete" => delete_message(c, t, filter.server.to_owned(), user, text[12..].to_owned()).await,
                        _ => false
                    }
                }
            });
        }
        true
    })
}

pub fn user_connected(t: Arc<Mutex<Data>>, mut c: Client, e: Server_Event) -> FutureValue<bool> {
    future(async move {
        let e = e.to_owned();
        if e.user.is_none() || e.user.as_ref().unwrap().id.is_none() {
            return true;
        }
        let user = e.user.unwrap();
        let server = e.server.clone();
        let server_path = &t.lock_async().await.path;
        let mailbox = load_mailbox(user.id(), server_path);
        if mailbox.messages.iter().any(|msg| msg.0) { // if at least one message is unread
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

fn delete_message(mut c: V1Client, mut t: Arc<Mutex<Data>>, server: Option<Server>, user: User, message: String) -> FutureValue<bool> {
    future(async move {
        let message = message.trim().to_owned();
        let server_path = &t.lock_async().await.path;
        let mut mailbox = load_mailbox(user.id(), server_path);
        if message.len() == 0 {
            for message in mailbox.messages.iter() {
                drop(std::fs::remove_file(message_path(server_path, mailbox.owner, message.2)));
            }
            mailbox.messages = vec![];
            drop(store_mailbox(&mailbox, server_path));
            drop(c.text_message_send(TextMessage {
                server: server,
                users: vec![user],
                text: Some("Emptied mailbox".to_string()),
                channels: vec![], trees: vec![], actor: None
            }).await);
            false
        } else if let Ok(index) = message.parse::<usize>() {
            if index < mailbox.messages.len() {
                let message = mailbox.messages.remove(index);
                drop(std::fs::remove_file(message_path(server_path, mailbox.owner, message.2)));
                drop(store_mailbox(&mailbox, server_path));
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

fn send_message(mut c: V1Client, mut t: Arc<Mutex<Data>>, server: Option<Server>, user: User, message: String) -> FutureValue<bool> {
    future(async move {
        let message = message.trim_start();
        if let Some(index) = message.find(" ") {
            let (recipient, message) = message.trim_start().split_at(index);
            let recipient = recipient.to_owned();
            let message = message.trim().to_owned();
            let server_path = t.lock_async().await.path.to_owned();
            if let Some(recipient_id) = user_id_from_name(c.clone(), server.clone(), &recipient).await {
                let mut mailbox = load_mailbox(recipient_id, &server_path);
                mailbox.owner = recipient_id; // if the mailbox is newly created, it will have the wrong owner id.
                let message = Message::new(user.id(), message.to_string());
                if mailbox.add_message(&server_path, message) {
                    let success_message = TextMessage {
                        server: server,
                        users: vec![user],
                        text: Some(format!(
                                "Message sent to <b><span style=\"color:#00aa7f\">{}</span></b>'s mailbox", 
                                recipient)),
                                channels: vec![], trees: vec![], actor: None
                    };
                    drop(store_mailbox(&mailbox, &server_path));
                    drop(c.text_message_send(success_message).await);
                } else {
                    let error_message = TextMessage {
                        server: server,
                        users: vec![user],
                        text: Some(format!(
                                "<b><span style=\"color:#00aa7f\">{}</span></b>'s mailbox is full so your message was not delivered", 
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
        }
        false
    })
}

fn read_message(mut c: V1Client, mut t: Arc<Mutex<Data>>, server: Option<Server>, user: User, message: String) -> FutureValue<bool> {
    future(async move {
        let message = message.trim().to_owned();
        let server_path = &t.lock_async().await.path;
        let mut mailbox = load_mailbox(user.id(), server_path);
        mailbox.owner = user.id(); // if the mailbox is newly created, it will have the wrong id.
        if let Ok(index) = message.parse::<usize>() {
            if let Some(message) = mailbox.read_message(server_path, index) {
                let text_message = TextMessage {
                    server: server.clone(),
                    users: vec![user],
                    text: message_string(c.clone(), server, message).await,
                    channels: vec![], trees: vec![], actor: None
                };
                drop(c.text_message_send(text_message).await);
                store_mailbox(&mailbox, server_path).unwrap();
            }
        } else {
            let mut mailbox_message = TextMessage::new();
            mailbox_message.set_server(server.clone());
            mailbox_message.set_users(vec![user]);
            mailbox_message.set_text(mailbox_string(c.clone(), server, mailbox).await);
            drop(c.text_message_send(&mailbox_message));
        }
        false
    })
}

async fn user_id_from_name(mut c: V1Client, server: Option<Server>, name: &str) -> Option<u32> {
    let query = Query {server: server, filter: Some(name.to_string())};
    if let Ok(response) = c.database_user_query(query).await {
        let list = response.into_inner();
        if list.users.len() > 0 {
            return Some(list.users[0].id());
        }
    }
    None
}

async fn user_name_from_id(mut c: V1Client, server: Option<Server>, id: u32) -> Option<String> {
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

async fn message_string(c: V1Client, server: Option<Server>, message: TextMessage) -> Option<String> {
    if let Some(name) = user_name_from_id(c, server, message.sender).await {
        return Some(format!("<h3>✉️ From: {}</h3>{}", name, message.contents));
    }
    None
}

async fn mailbox_string(c: Client, server: Option<Server>, mailbox: Mailbox) -> String {
    let mut mailbox_string = "<tt><h3>📬 Your Mailbox:</h3>".to_string();
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
    mailbox_string.push_str("<br/><sup>type <b>!mail</b> for instructions</sup></tt>");
    mailbox_string
}

fn load_mailbox(owner: u32, server_path: &PathBuf) -> Mailbox {
    Mailbox::load(server_path.join(format!("mailboxes/{}/mailbox.toml", owner)))
}

fn store_mailbox(mailbox: &Mailbox, server_path: &PathBuf) -> Result<(), ConfyError> {
    mailbox.store(server_path.join(format!("mailboxes/{}/mailbox.toml", mailbox.owner)))
}

fn message_path(server_path: &PathBuf, owner: u32, name: u32) -> PathBuf {
    server_path.join(format!("mailboxes/{}/message_{}.toml", owner, name))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Mailbox {
    owner: u32,
    messages: Vec<(bool, u32, u32)>
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

    pub fn add_message(&mut self, server_path: &PathBuf, message: Message) -> bool {
        if self.messages.len() > MAX_MESSAGES {
            return false;
        }
        let message_name = message.unique_message_name(self.owner, server_path);
        let message_path = message_path(server_path, self.owner, message_name);
        self.messages.push((true, message.sender, message_name));
        drop(message.store(&message_path));
        true
    }

    pub fn read_message(&mut self, server_path: &PathBuf, index: usize) -> Option<Message> {
        if let Some(message) = self.messages.get_mut(index) {
            message.0 = false;
            return Some(Message::load(&message_path(server_path, self.owner, message.2)));
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

    pub fn unique_message_name(&self, owner: u32, server_path: &PathBuf) -> u32 {
        let mut rng = rand::thread_rng();
        let mut name = 0;
        while message_path(server_path, owner, name).exists() {
            name = rng.gen::<u32>();
        }
        name
    }
}
