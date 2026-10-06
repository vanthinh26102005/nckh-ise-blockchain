use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendFailure {
    Saturated,
    Stopped,
    Disconnected,
}

pub fn send_before_deadline<T>(
    sender: &SyncSender<T>,
    mut item: T,
    stop: &AtomicBool,
    timeout: Duration,
) -> Result<(), (T, SendFailure)> {
    let deadline = Instant::now() + timeout;
    loop {
        if stop.load(Ordering::Acquire) {
            return Err((item, SendFailure::Stopped));
        }
        match sender.try_send(item) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(value)) => {
                return Err((value, SendFailure::Disconnected))
            }
            Err(TrySendError::Full(value)) => item = value,
        }
        if Instant::now() >= deadline {
            return Err((item, SendFailure::Saturated));
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_queue_returns_saturation_and_stop_without_a_consumer_join() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let stop = AtomicBool::new(false);
        sender.send(1).unwrap();
        assert_eq!(
            send_before_deadline(&sender, 2, &stop, Duration::from_millis(1)),
            Err((2, SendFailure::Saturated))
        );
        assert_eq!(receiver.recv().unwrap(), 1);
        assert_eq!(
            send_before_deadline(&sender, 3, &stop, Duration::ZERO),
            Ok(())
        );
        stop.store(true, Ordering::Release);
        assert_eq!(
            send_before_deadline(&sender, 4, &stop, Duration::from_secs(10)),
            Err((4, SendFailure::Stopped))
        );
        stop.store(false, Ordering::Release);
        drop(receiver);
        assert_eq!(
            send_before_deadline(&sender, 5, &stop, Duration::from_secs(10)),
            Err((5, SendFailure::Disconnected))
        );
    }
}
