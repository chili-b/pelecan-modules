# Mail

This module adds a simple persistent messaging system to your Mumble server.

* Only responds to registered users
* Stores messages in the local data directory (`~/.local/share/pelecan/<server name>/mailboxes/` on Linux)

## Features

* Notifies users if they have unread messages when they connect
* Highlights unread messages in your mailbox so you can easily see what's new
* Hides mail messages so you can type them anywhere without worrying about other users seeing them

## Usage

Type `!mail` to see this help in mumble

- `!mail send <recipient name> <message>` - sends a message to a user
- `!mail read` - displays your mailbox
- `!mail read <number>` - displays a specific message from your mailbox
- `!mail delete` - empties your mailbox
- `!mail delete <number>` - deletes a specific message from your mailbox
