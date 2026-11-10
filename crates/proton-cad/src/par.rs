//! Work shared across the machine's cores: each item done once, by
//! whichever thread is free, the results kept in the items' order — so
//! what comes out is the same as done one by one.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// `f` of each of `items`, in their order, on as many threads as there
/// are cores (one, for one item).
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let out: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                let r = f(&items[i]);
                *out[i].lock().expect("a result slot") = Some(r);
            });
        }
    });
    out.into_iter().map(|m| m.into_inner().expect("a result slot").expect("every item done")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn results_come_back_in_order() {
        let items: Vec<u64> = (0..1000).collect();
        assert_eq!(super::map(&items, |x| x * x), items.iter().map(|x| x * x).collect::<Vec<_>>());
        assert!(super::map(&Vec::<u8>::new(), |x| *x).is_empty());
    }
}
