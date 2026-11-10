use std::time::Duration;

use crossterm::event::{Event as CtEvent, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{Interval, MissedTickBehavior, interval};

use crate::app::Cmd;
use crate::mopidy::MpdEvent;

pub enum AppEvent {
    Key(KeyEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
    Tick,
    Mpd(MpdEvent),
    /// Command injected from outside the input layer (today: MPRIS).
    Cmd(Cmd),
}

pub struct Events {
    input: EventStream,
    tick: Interval,
    tick_ms: u64,
    mpd: broadcast::Receiver<MpdEvent>,
    cmds: mpsc::UnboundedReceiver<Cmd>,
}

impl Events {
    pub fn new(
        mpd: broadcast::Receiver<MpdEvent>,
        cmds: mpsc::UnboundedReceiver<Cmd>,
        tick_ms: u64,
    ) -> Self {
        let mut t = interval(Duration::from_millis(tick_ms));
        t.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self { input: EventStream::new(), tick: t, tick_ms, mpd, cmds }
    }

    /// Change the tick period (no-op when it is already `tick_ms`).
    pub fn set_tick_ms(&mut self, tick_ms: u64) {
        if tick_ms == self.tick_ms {
            return;
        }
        let period = Duration::from_millis(tick_ms);
        let mut t = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        t.set_missed_tick_behavior(MissedTickBehavior::Skip);
        self.tick = t;
        self.tick_ms = tick_ms;
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
                Some(cmd) = self.cmds.recv() => return AppEvent::Cmd(cmd),
            }
        }
    }
}
