use std::time::Duration;

use crossterm::event::{Event as CtEvent, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use tokio::sync::broadcast;
use tokio::time::{Interval, MissedTickBehavior, interval};

use crate::mopidy::MpdEvent;

#[derive(Debug, Clone)]
pub enum AppEvent {
    Key(KeyEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
    Tick,
    Mpd(MpdEvent),
}

pub struct Events {
    input: EventStream,
    tick: Interval,
    mpd: broadcast::Receiver<MpdEvent>,
}

impl Events {
    pub fn new(mpd: broadcast::Receiver<MpdEvent>, tick_ms: u64) -> Self {
        let mut t = interval(Duration::from_millis(tick_ms));
        t.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self { input: EventStream::new(), tick: t, mpd }
    }

    pub async fn next(&mut self) -> AppEvent {
        loop {
            tokio::select! {
                biased;
                _ = self.tick.tick() => return AppEvent::Tick,
                maybe = self.input.next() => {
                    let Some(Ok(ev)) = maybe else { continue };
                    match ev {
                        CtEvent::Key(k) if k.kind == KeyEventKind::Press => return AppEvent::Key(k),
                        CtEvent::Resize(w, h) => return AppEvent::Resize(w, h),
                        _ => continue,
                    }
                }
                ev = self.mpd.recv() => {
                    match ev {
                        Ok(ev) => return AppEvent::Mpd(ev),
                        // Lagged or closed: keep looping.
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => continue,
                    }
                }
            }
        }
    }
}
