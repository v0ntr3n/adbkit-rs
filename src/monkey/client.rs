//! A client for the `monkey` utility running on a device.
//!
//! Ported from `lib/monkey/client.js` and `lib/monkey/multi.js`. The TypeScript
//! client is an `EventEmitter`; here the events arrive over a channel.

use std::sync::Arc;

use std::sync::Mutex;

use tokio::io::{AsyncReadExt, AsyncWriteExt, WriteHalf};
use tokio::sync::{mpsc, Notify};

use crate::connection::AdbStream;
use crate::error::{Error, Result};
use crate::monkey::api::Api;
use crate::monkey::parser::Parser as MonkeyParser;
use crate::monkey::queue::Queue;
use crate::monkey::reply::Reply;

/// An event emitted by a [`Client`].
#[derive(Debug)]
pub enum ClientEvent {
    /// A reply to a command.
    Reply {
        /// The reply.
        reply: Reply,
        /// The command it belongs to.
        command: String,
    },
    /// An error occurred.
    Error(Error),
    /// The connection ended.
    End,
    /// The connection was closed by the remote end.
    Finish,
}

/// A client for the `monkey` utility.
///
/// Ported from the `Client` class of `lib/monkey/client.js`.
pub struct Client<S>
where
    S: AdbStream,
{
    stream: WriteHalf<S>,
    queue: Arc<Mutex<Queue<String>>>,
    events: mpsc::Receiver<ClientEvent>,
    stop: Arc<Notify>,
}

impl<S> Client<S>
where
    S: AdbStream,
{
    /// Connects the client to the given stream.
    ///
    /// Ported from `Client.connect()`.
    pub fn connect(stream: S) -> Self {
        let (mut reader, writer) = tokio::io::split(stream);
        let (tx, rx) = mpsc::channel(256);
        let stop = Arc::new(Notify::new());
        let stop_clone = stop.clone();
        let commands: Arc<Mutex<Queue<String>>> = Arc::new(Mutex::new(Queue::new()));
        let reader_commands = Arc::clone(&commands);

        tokio::spawn(async move {
            let mut parser = MonkeyParser::new();
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                tokio::select! {
                    result = reader.read(&mut buf) => {
                        match result {
                            Ok(0) => {
                                let _ = tx.send(ClientEvent::End).await;
                                break;
                            }
                            Ok(read) => {
                                let replies = match parser.parse(&buf[..read]) {
                                    Ok(replies) => replies,
                                    Err(err) => {
                                        let _ = tx.send(ClientEvent::Error(err)).await;
                                        continue;
                                    }
                                };
                                for reply in replies {
                                    // The guard has to be dropped before awaiting.
                                    let command = reader_commands
                                        .lock()
                                        .ok()
                                        .and_then(|mut queue| queue.dequeue());
                                    match command {
                                        Some(command) => {
                                            let _ = tx
                                                .send(ClientEvent::Reply { reply, command })
                                                .await;
                                        }
                                        None => {
                                            let _ = tx
                                                .send(ClientEvent::Error(Error::Device(
                                                    "Command queue depleted, but replies still coming in"
                                                        .to_string(),
                                                )))
                                                .await;
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                let _ = tx.send(ClientEvent::Error(Error::Io(err))).await;
                                break;
                            }
                        }
                    }
                    _ = stop_clone.notified() => break,
                }
            }
            let _ = tx.send(ClientEvent::Finish).await;
        });

        Client {
            stream: writer,
            queue: commands,
            events: rx,
            stop,
        }
    }

    /// Waits for the next event.
    pub async fn next(&mut self) -> Option<ClientEvent> {
        self.events.recv().await
    }

    /// Enqueues a command that is waiting for a reply.
    fn enqueue(&self, command: String) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.enqueue(command);
        }
    }

    /// Sends a command and waits for its reply.
    ///
    /// Ported from `Client.send()`.
    pub async fn send(&mut self, command: impl Into<String>) -> Result<Reply> {
        let command = command.into();
        let mut line = command.clone().into_bytes();
        line.push(b'\n');

        self.enqueue(command);
        self.stream.write_all(&line).await.map_err(Error::from)?;
        self.stream.flush().await.map_err(Error::from)?;

        match self.events.recv().await {
            Some(ClientEvent::Reply { reply, .. }) => Ok(reply),
            Some(ClientEvent::Error(err)) => Err(err),
            Some(ClientEvent::End) | Some(ClientEvent::Finish) | None => {
                Err(Error::Device("The connection ended".to_string()))
            }
        }
    }

    /// Sends several commands and collects their replies.
    ///
    /// Ported from the `Array` branch of `Client.send()`.
    pub async fn send_all(&mut self, commands: &[String]) -> Result<Vec<Reply>> {
        let mut line = commands.join("\n").into_bytes();
        line.push(b'\n');

        for command in commands {
            self.enqueue(command.clone());
        }
        self.stream.write_all(&line).await.map_err(Error::from)?;
        self.stream.flush().await.map_err(Error::from)?;

        let mut replies = Vec::with_capacity(commands.len());
        for _ in commands {
            match self.events.recv().await {
                Some(ClientEvent::Reply { reply, .. }) => replies.push(reply),
                Some(ClientEvent::Error(err)) => return Err(err),
                _ => return Err(Error::Device("The connection ended".to_string())),
            }
        }
        Ok(replies)
    }

    /// Creates a [`Multi`] batch.
    ///
    /// Ported from `Client.multi()`.
    pub fn multi(&mut self) -> Multi<'_, S> {
        Multi::new(self)
    }

    /// Ends the connection.
    ///
    /// Ported from `Client.end()`.
    pub async fn end(&mut self) {
        self.stop.notify_waiters();
        let _ = self.stream.shutdown().await;
    }

    /// `key down <keyCode>`
    pub async fn key_down(&mut self, key_code: u32) -> Result<Reply> {
        self.send(Api::key_down(key_code)).await
    }

    /// `key up <keyCode>`
    pub async fn key_up(&mut self, key_code: u32) -> Result<Reply> {
        self.send(Api::key_up(key_code)).await
    }

    /// `touch down <x> <y>`
    pub async fn touch_down(&mut self, x: i32, y: i32) -> Result<Reply> {
        self.send(Api::touch_down(x, y)).await
    }

    /// `touch up <x> <y>`
    pub async fn touch_up(&mut self, x: i32, y: i32) -> Result<Reply> {
        self.send(Api::touch_up(x, y)).await
    }

    /// `touch move <x> <y>`
    pub async fn touch_move(&mut self, x: i32, y: i32) -> Result<Reply> {
        self.send(Api::touch_move(x, y)).await
    }

    /// `trackball <dx> <dy>`
    pub async fn trackball(&mut self, dx: i32, dy: i32) -> Result<Reply> {
        self.send(Api::trackball(dx, dy)).await
    }

    /// `flip open`
    pub async fn flip_open(&mut self) -> Result<Reply> {
        self.send(Api::flip_open()).await
    }

    /// `flip close`
    pub async fn flip_close(&mut self) -> Result<Reply> {
        self.send(Api::flip_close()).await
    }

    /// `wake`
    pub async fn wake(&mut self) -> Result<Reply> {
        self.send(Api::wake()).await
    }

    /// `tap <x> <y>`
    pub async fn tap(&mut self, x: i32, y: i32) -> Result<Reply> {
        self.send(Api::tap(x, y)).await
    }

    /// `press <keyCode>`
    pub async fn press(&mut self, key_code: u32) -> Result<Reply> {
        self.send(Api::press(key_code)).await
    }

    /// `type <str>`
    pub async fn type_str(&mut self, value: &str) -> Result<Reply> {
        self.send(Api::type_str(value)).await
    }

    /// `listvar`
    pub async fn list(&mut self) -> Result<Reply> {
        self.send(Api::list()).await
    }

    /// `getvar <name>`
    pub async fn get(&mut self, name: &str) -> Result<Reply> {
        self.send(Api::get(name)).await
    }

    /// `quit`
    pub async fn quit(&mut self) -> Result<Reply> {
        self.send(Api::quit()).await
    }

    /// `done`
    pub async fn done(&mut self) -> Result<Reply> {
        self.send(Api::done()).await
    }

    /// `sleep <ms>`
    pub async fn sleep(&mut self, ms: u32) -> Result<Reply> {
        self.send(Api::sleep(ms)).await
    }
}

/// A batch of commands that are sent together.
///
/// Ported from the `Multi` class of `lib/monkey/multi.js`.
pub struct Multi<'a, S>
where
    S: AdbStream,
{
    client: &'a mut Client<S>,
    commands: Vec<String>,
    sent: bool,
}

impl<'a, S> Multi<'a, S>
where
    S: AdbStream,
{
    /// Creates a new batch.
    pub fn new(client: &'a mut Client<S>) -> Self {
        Multi {
            client,
            commands: Vec::new(),
            sent: false,
        }
    }

    /// Adds a command to the batch.
    ///
    /// Ported from `Multi.send()`.
    pub fn send(&mut self, command: impl Into<String>) -> &mut Self {
        self.forbid_reuse();
        self.commands.push(command.into());
        self
    }

    /// Sends all the commands and collects their replies.
    ///
    /// Ported from `Multi.execute()`.
    pub async fn execute(&mut self) -> Result<Vec<Reply>> {
        self.forbid_reuse();
        self.sent = true;
        let replies = self.client.send_all(&self.commands).await?;
        let errors: Vec<String> = replies
            .iter()
            .zip(&self.commands)
            .filter(|(reply, _)| reply.is_error())
            .map(|(reply, command)| {
                format!("{}: {}", command, reply.value.clone().unwrap_or_default())
            })
            .collect();
        if !errors.is_empty() {
            return Err(Error::Device(errors.join(", ")));
        }
        Ok(replies)
    }

    fn forbid_reuse(&mut self) {
        if self.sent {
            panic!("Reuse not supported");
        }
    }
}

/// Options for [`crate::monkey::Monkey::connect`].
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    /// The host to connect to.
    pub host: String,
    /// The port to connect to.
    pub port: u16,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            host: "127.0.0.1".to_string(),
            port: 1080,
        }
    }
}

/// Connects to a `monkey` utility over TCP.
///
/// Ported from the `Connection` class of `lib/monkey/connection.js`.
pub async fn connect(options: ConnectOptions) -> Result<Client<tokio::net::TcpStream>> {
    let stream = tokio::net::TcpStream::connect((options.host.as_str(), options.port)).await?;
    stream.set_nodelay(true).ok();
    Ok(Client::connect(stream))
}

/// Connects a client to an already established stream.
///
/// Ported from `Monkey.connectStream()`.
pub fn connect_stream<S: AdbStream>(stream: S) -> Client<S> {
    Client::connect(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monkey::reply::ReplyType;

    #[tokio::test]
    async fn test_send_writes_the_command() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut monkey = Client::connect(client);

        // Nothing answers, so the command itself is all that can be observed.
        let sent =
            tokio::time::timeout(std::time::Duration::from_millis(200), monkey.tap(10, 20)).await;
        assert!(sent.is_err(), "no reply is expected");

        let mut buf = vec![0u8; 32];
        let read = tokio::io::AsyncReadExt::read(&mut server, &mut buf)
            .await
            .unwrap();
        assert_eq!(&buf[..read], b"tap 10 20\n");
    }

    #[tokio::test]
    async fn test_send_receives_a_reply() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut monkey = Client::connect(client);

        // Reply as soon as the command arrives.
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut buf = vec![0u8; 32];
            let _ = tokio::io::AsyncReadExt::read(&mut server, &mut buf).await;
            let _ = server.write_all(b"OK\n").await;
        });

        let reply = monkey.tap(10, 20).await.unwrap();
        assert_eq!(reply.r#type, ReplyType::Ok);
        assert_eq!(reply.value, None);
    }

    #[tokio::test]
    async fn test_default_connect_options() {
        let options = ConnectOptions::default();
        assert_eq!(options.host, "127.0.0.1");
        assert_eq!(options.port, 1080);
    }

    #[test]
    fn test_reply_type_display() {
        assert_eq!(ReplyType::Ok.to_string(), "OK");
        assert_eq!(ReplyType::Error.to_string(), "ERROR");
    }
}
