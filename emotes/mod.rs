use std::collections::{VecDeque, HashMap};
use crate::{DataMutex, Client, future_from_bool};
use std::path::PathBuf;
use crate::FutureBool;
use crate::text_message::{Filter};
use super::Data;
use regex::Regex;
use std::process::Command;

// emotes are cached so that using the same emote multiple times in a message does not cause that
// many reads from the filesystem to happen.
const MAX_CACHED_EMOTES: usize = 10;
// since each image is inlined as base64 encoded data, you should set a cap on how
// many emotes can be in a message to combat spam
const MAX_EMOTES_PER_MESSAGE: usize = 5;
// emote height in pixels
const EMOTE_HEIGHT: u8 = 25;
const EMOTE_DELIMITER: char = ':';
// restrictions on what constitutes a valid emote name
const EMOTE_REGEX: &'static str = r"[a-z0-9]+";

#[derive(Clone)]
pub struct EmoteCache {
    cached_emotes: VecDeque<String>,
    base64_cache: HashMap<String, String>
}

impl EmoteCache {
    pub fn new() -> Self {
        Self {
            cached_emotes: VecDeque::new(),
            base64_cache: HashMap::new(),
        }
    }

    pub fn get_emote(&mut self, name: &str, dir: &PathBuf) -> Option<String> {
        if self.base64_cache.contains_key(name) {
            // move the emote to the front of the cache
            self.cache(name.to_owned(), String::with_capacity(0));
            Some(self.base64_cache.get(name).unwrap().to_string())
        } else if let Some(base64) = load_emote(dir, name) {
			self.cache(name.to_owned(), base64.clone());
            Some(base64)
        } else {
            None
        }
    }

    fn cache(&mut self, name: String, base64: String) {
        if let Some(index) = self.cached_emotes.iter().position(|c| c == &name) {
            self.cached_emotes.remove(index);
        } else {
            self.base64_cache.insert(name.clone(), base64);
            if self.cached_emotes.len() > MAX_CACHED_EMOTES {
                if let Some(c) = self.cached_emotes.pop_back() {
                    self.base64_cache.remove(&c);
                }
            }
        }
        self.cached_emotes.push_front(name);
    }
}

fn load_emote(dir: &PathBuf, name: &str) -> Option<String> {
    let png_path = dir.join(format!("{}.png", name));
    if png_path.exists() {
		return Some(image_html(base64_encode(&png_path)?));
    }
    let txt_path = dir.join(format!("{}.txt", name));
    if txt_path.exists() {
        return Some(String::from_utf8_lossy(&std::fs::read(txt_path).ok()?).to_string());
    }
	None
}

fn base64_encode(path: &PathBuf) -> Option<String> {
	if let Ok(path) = path.canonicalize() {
		if let Some(path_str) = path.to_str() {
			let base64 = Command::new("base64")
						.arg(path_str)
						.output()
						.ok()?;
            return Some(String::from_utf8_lossy(&base64.stdout).to_string());
		}
	}
    None
}

fn image_html(base64: String) -> String {
    format!("<img height=\"{}\" src=\"data:image/png;base64,{}\"/>", EMOTE_HEIGHT, base64)
}


pub fn chat_filter(mut t: DataMutex<Data>, _c: Client, filter: &mut Filter) -> FutureBool {
    if filter.message.is_none() || filter.message.as_ref().unwrap().text.is_none() {
        return future_from_bool(true);
    }
    let text = filter.message.as_ref().unwrap().text.as_ref().unwrap().to_owned();
    let dir = t.lock().path.join("emotes");
    if !std::fs::create_dir_all(&dir).is_ok() {
        return future_from_bool(true);
    }

    let regex = if let Ok(regex) = Regex::new(EMOTE_REGEX) {
        regex
    } else {
        eprintln!("Emote regex is not valid!");
        return future_from_bool(true);
    };

    let mut text_iter = text.chars().enumerate();
    let mut new_message = String::with_capacity(text.len());
    let mut start_index = 0;
    let mut end_index = 0;
    let mut num_emotes = 0;

    while let Some((index, chr)) = text_iter.next() {
        // iterate until we find the emote delimiter
        if chr == EMOTE_DELIMITER {
            new_message.push_str(&text[start_index..index]);
            start_index = index;
            // iterate until we find a closing delimiter
            while let Some((index, chr)) = text_iter.next() {
                end_index = index;
                if chr == EMOTE_DELIMITER {
                    let emote_name = &text[(start_index + 1)..index];
                    if regex.is_match(emote_name) {
                        num_emotes += 1;
                        let emote_cache = &mut t.lock().volatile_data.emote_cache;
                        if let Some(emote) = emote_cache.get_emote(emote_name, &dir) {
                            new_message.push_str(&emote);
                        }
                        start_index = index + 1;
                        break;
                    }
                }
            }
            if num_emotes > MAX_EMOTES_PER_MESSAGE {
                break;
            }
            new_message.push_str(&text[start_index..(end_index + 1)])
        }
    }
    new_message.push_str(&text[start_index..]);

    if num_emotes <= MAX_EMOTES_PER_MESSAGE && num_emotes != 0 {
        let mut text_message = filter.message.as_ref().unwrap().to_owned();
        text_message.text = Some(new_message);
        filter.message = Some(text_message);
    }

    future_from_bool(true)
}
