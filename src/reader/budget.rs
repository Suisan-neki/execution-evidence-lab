//! 子プロセスの強制終了に代わり、解析の反復で同じ期限を確認する。
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub(super) struct Budget {
    pub until: Instant,
    stopped: Cell<bool>,
}
impl Budget {
    pub fn new() -> Self {
        Self {
            until: Instant::now() + Duration::from_secs(30),
            stopped: Cell::new(false),
        }
    }
    pub fn check(&self) -> bool {
        if Instant::now() >= self.until {
            self.stopped.set(true);
        }
        !self.stopped.get()
    }
    pub fn stop(&self) {
        self.stopped.set(true);
    }
}
