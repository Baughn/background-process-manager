use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Resize(u16, u16),
}

#[derive(Debug)]
pub struct EventHandler {
    #[allow(dead_code)]
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        let event_sender = sender.clone();
        tokio::spawn(async move {
            loop {
                // Wait for crossterm event with timeout
                if event::poll(tick_rate).unwrap() {
                    match event::read().unwrap() {
                        CrosstermEvent::Key(key) => {
                            if key.kind == KeyEventKind::Press {
                                event_sender.send(Event::Key(key)).ok();
                            }
                        }
                        CrosstermEvent::Resize(width, height) => {
                            event_sender.send(Event::Resize(width, height)).ok();
                        }
                        _ => {}
                    }
                } else {
                    // Timeout - send tick
                    event_sender.send(Event::Tick).ok();
                }
            }
        });

        Self { sender, receiver }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.receiver.recv().await
    }
}
