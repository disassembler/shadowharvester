use std::thread;
use std::time::Duration;

pub struct Backoff {
    pub cur: f64,
    pub min: f64,
    pub max: f64,
    pub factor: f64,
}

impl Backoff {
    pub fn new(min: u64, max: u64, factor: f64) -> Self {
        Self {
            cur: min as f64,
            min: min as f64,
            max: max as f64,
            factor,
        }
    }

    pub fn sleep(&mut self) {
        let secs = self.cur.min(self.max);
        println!("sleep {secs:.0}s");
        thread::sleep(Duration::from_secs_f64(secs));
        self.cur = (self.cur * self.factor).min(self.max);
    }

    pub fn reset(&mut self) {
        self.cur = self.min;
    }
}
