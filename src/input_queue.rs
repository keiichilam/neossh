//! Ordered input with a byte budget instead of a message-count budget.
use std::{collections::VecDeque, sync::Mutex};
use zeroize::Zeroize;
const LIMIT: usize = 8 * 1024 * 1024;
#[derive(Default)]
struct State {
    messages: VecDeque<Vec<u8>>,
    bytes: usize,
    closed: bool,
}
#[derive(Default)]
pub struct Queue(Mutex<State>);
impl Queue {
    pub fn push(&self, mut bytes: Vec<u8>) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|_| "Input queue unavailable.")?;
        let error = if state.closed {
            Some("Session is disconnected.")
        } else if bytes.len() > 1024 * 1024 {
            Some("Input is larger than 1 MiB. Send it in smaller parts.")
        } else if bytes.len() > LIMIT - state.bytes {
            Some("Input queue is full (8 MiB). This input was not sent; wait and retry.")
        } else {
            None
        };
        if let Some(error) = error {
            bytes.zeroize();
            return Err(error.into());
        }
        state.bytes += bytes.len();
        // Coalesce small keystrokes so the byte limit also bounds allocation overhead.
        if let Some(last) = state
            .messages
            .back_mut()
            .filter(|last| last.len() + bytes.len() <= 8192)
        {
            last.extend_from_slice(&bytes);
            bytes.zeroize();
        } else if !bytes.is_empty() {
            state.messages.push_back(bytes);
        }
        Ok(())
    }
    pub fn pop(&self) -> Result<Option<Vec<u8>>, String> {
        let mut state = self.0.lock().map_err(|_| "Input queue unavailable.")?;
        let bytes = state.messages.pop_front();
        if let Some(bytes) = &bytes {
            state.bytes -= bytes.len();
        }
        Ok(bytes)
    }
    pub fn close(&self) {
        if let Ok(mut state) = self.0.lock() {
            for bytes in &mut state.messages {
                bytes.zeroize();
            }
            state.messages.clear();
            state.bytes = 0;
            state.closed = true;
        }
    }
}
impl Drop for Queue {
    fn drop(&mut self) {
        self.close();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bursts_are_ordered_bounded_and_rejected_atomically() {
        let queue = Queue::default();
        for index in 0..1000u32 {
            queue.push(index.to_be_bytes().to_vec()).unwrap();
        }
        assert_eq!(
            queue.pop().unwrap().unwrap(),
            (0..1000u32).flat_map(u32::to_be_bytes).collect::<Vec<_>>()
        );
        for index in 0..8u8 {
            queue.push(vec![index; 1024 * 1024]).unwrap();
        }
        assert!(queue.push(vec![99]).unwrap_err().contains("not sent"));
        for index in 0..8u8 {
            assert_eq!(queue.pop().unwrap().unwrap(), vec![index; 1024 * 1024]);
        }
        assert!(queue.pop().unwrap().is_none());
        queue.push(b"queued".to_vec()).unwrap();
        queue.close();
        assert!(queue.pop().unwrap().is_none());
        assert!(queue.push(vec![1]).is_err());
    }
}
