//! 優先度付きジョブキュー + 固定数のワーカースレッド。各ジョブは catch_unwind で隔離する。

use std::collections::BinaryHeap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::thread;

use parking_lot::{Condvar, Mutex};

type Job = Box<dyn FnOnce() + Send>;

struct Item {
    prio: u8,
    seq: u64,
    job: Job,
}

impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        (self.prio, self.seq) == (o.prio, o.seq)
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    /// 優先度が高いものを先に、同じなら投入順（seq が小さい方）を先に取り出す
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.prio.cmp(&o.prio).then(o.seq.cmp(&self.seq))
    }
}

#[derive(Default)]
struct State {
    heap: BinaryHeap<Item>,
    seq: u64,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

pub struct JobQueue {
    shared: Arc<Shared>,
}

impl JobQueue {
    pub fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
        });
        for n in 0..workers.max(1) {
            let sh = Arc::clone(&shared);
            let _ = thread::Builder::new()
                .name(format!("pv-worker-{n}"))
                .spawn(move || worker(sh));
        }
        Self { shared }
    }

    pub fn submit(&self, prio: u8, job: impl FnOnce() + Send + 'static) {
        let mut st = self.shared.state.lock();
        let seq = st.seq;
        st.seq += 1;
        st.heap.push(Item {
            prio,
            seq,
            job: Box::new(job),
        });
        drop(st);
        self.shared.cv.notify_one();
    }
}

impl Drop for JobQueue {
    fn drop(&mut self) {
        let mut st = self.shared.state.lock();
        st.shutdown = true;
        st.heap.clear();
        drop(st);
        self.shared.cv.notify_all();
    }
}

fn worker(sh: Arc<Shared>) {
    loop {
        let item = {
            let mut st = sh.state.lock();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some(it) = st.heap.pop() {
                    break it;
                }
                sh.cv.wait(&mut st);
            }
        };
        if catch_unwind(AssertUnwindSafe(item.job)).is_err() {
            tracing::error!("job panicked; worker continues");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn higher_priority_runs_first() {
        let q = JobQueue::new(1);
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let (out_tx, out_rx) = mpsc::channel();
        // ワーカーを塞いでいる間に優先度の異なるジョブを積む
        q.submit(9, move || {
            gate_rx.recv().unwrap();
        });
        thread::sleep(Duration::from_millis(50));
        for (p, name) in [(1u8, "low"), (3, "high"), (2, "mid"), (3, "high2")] {
            let tx = out_tx.clone();
            q.submit(p, move || tx.send(name).unwrap());
        }
        gate_tx.send(()).unwrap();
        let got: Vec<_> = (0..4)
            .map(|_| out_rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(got, ["high", "high2", "mid", "low"]);
    }

    #[test]
    fn panicking_job_does_not_kill_worker() {
        let q = JobQueue::new(1);
        let (tx, rx) = mpsc::channel();
        q.submit(1, || panic!("boom"));
        q.submit(1, move || tx.send(42).unwrap());
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), 42);
    }
}
