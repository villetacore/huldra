//! The kernel's random number generator (`/dev/urandom`, `getrandom`).
//!
//! Entropy goes into a SHA-256 pool: RDSEED/RDRAND when the CPU has them,
//! the jitter of the time-stamp counter around memory accesses at boot,
//! the wall clock, and the arrival time of every interrupt. Output comes
//! from ChaCha20 keyed by the pool; after every request the generator
//! replaces its key with fresh keystream ("fast key erasure"), so earlier
//! output cannot be reconstructed from a later state. The key is mixed
//! with the pool again every few seconds of interrupts.

use crate::arch::cpu::rdtsc;
use crate::sync::SpinLock;
use core::arch::x86_64::__cpuid;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use huldra_crypto::aead::chacha20_block;
use huldra_crypto::sha::{Hash, Sha256};

struct Generator {
    key: [u8; 32],
    pool: Sha256,
    /// Events hashed into the pool since the last reseed.
    pooled: usize,
}

static RNG: SpinLock<Option<Generator>> = SpinLock::new(None);

/// Interrupt timings collected without locking, folded in on use.
static EVENTS: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];
static EVENT_COUNT: AtomicUsize = AtomicUsize::new(0);

fn rdrand64(seed: bool) -> Option<u64> {
    let leaf1 = __cpuid(1);
    let leaf7 = __cpuid(7);
    let have = if seed { leaf7.ebx & (1 << 18) != 0 } else { leaf1.ecx & (1 << 30) != 0 };
    if !have {
        return None;
    }
    for _ in 0..10 {
        let (v, ok): (u64, u8);
        unsafe {
            if seed {
                core::arch::asm!("rdseed {}", "setc {}", out(reg) v, out(reg_byte) ok, options(nomem, nostack));
            } else {
                core::arch::asm!("rdrand {}", "setc {}", out(reg) v, out(reg_byte) ok, options(nomem, nostack));
            }
        }
        if ok == 1 {
            return Some(v);
        }
    }
    None
}

/// Timing jitter: how long small chunks of memory traffic take varies
/// with caches, the host and the emulator.
fn jitter(pool: &mut Sha256, rounds: usize) {
    let mut scratch = [0u64; 64];
    let mut last = rdtsc();
    for i in 0..rounds {
        for j in 0..8 {
            let k = (last as usize).wrapping_add(i * 7 + j) % scratch.len();
            scratch[k] = scratch[k].wrapping_mul(6364136223846793005).wrapping_add(last);
        }
        let now = rdtsc();
        pool.update(&(now.wrapping_sub(last) ^ scratch[i % 64]).to_le_bytes());
        last = now;
    }
}

/// Seeds the generator; called once early during boot.
pub fn init() {
    let mut pool = Sha256::new();
    pool.update(b"huldra random pool");
    let mut hw = 0;
    for _ in 0..8 {
        if let Some(v) = rdrand64(true).or_else(|| rdrand64(false)) {
            pool.update(&v.to_le_bytes());
            hw += 1;
        }
    }
    pool.update(&crate::time::now().to_le_bytes());
    jitter(&mut pool, 4096);
    let key = pool.clone().finish();
    kinfo!("random: seeded ({})", if hw > 0 { "RDSEED/RDRAND, timer jitter" } else { "timer jitter, no hardware RNG" });
    *RNG.lock() = Some(Generator { key, pool, pooled: 0 });
}

/// Records the time of an event (an interrupt). Lock-free and cheap.
pub fn add_event(source: u64) {
    let n = EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
    let slot = &EVENTS[n % EVENTS.len()];
    slot.store(slot.load(Ordering::Relaxed).rotate_left(7) ^ rdtsc() ^ source << 56, Ordering::Relaxed);
}

/// Fills `buf` with random bytes.
pub fn fill(buf: &mut [u8]) {
    let mut guard = RNG.lock();
    let g = match guard.as_mut() {
        Some(g) => g,
        None => {
            drop(guard);
            init();
            guard = RNG.lock();
            guard.as_mut().unwrap()
        }
    };
    // Fold in interrupt timings, and reseed the key now and then.
    for e in EVENTS.iter() {
        g.pool.update(&e.load(Ordering::Relaxed).to_le_bytes());
    }
    g.pool.update(&rdtsc().to_le_bytes());
    g.pooled += 1;
    if g.pooled >= 64 || EVENT_COUNT.load(Ordering::Relaxed) % 1024 == 0 {
        let mut h = g.pool.clone();
        h.update(&g.key);
        g.key = h.finish();
        g.pooled = 0;
    }
    let nonce = [0u8; 12];
    let mut counter = 1u32;
    for chunk in buf.chunks_mut(64) {
        let block = chacha20_block(&g.key, counter, &nonce);
        chunk.copy_from_slice(&block[..chunk.len()]);
        counter += 1;
    }
    // Fast key erasure: block 0 becomes the next key.
    let next = chacha20_block(&g.key, 0, &nonce);
    g.key.copy_from_slice(&next[..32]);
}

pub const TESTS: &[crate::ktest::Test] = ktests![tests::output];

mod tests {
    pub fn output() {
        let (mut a, mut b) = ([0u8; 100], [0u8; 100]);
        super::fill(&mut a);
        super::fill(&mut b);
        assert_ne!(a, b);
        assert!(a.iter().any(|&x| x != 0));
        // Roughly balanced bits over a larger sample.
        let mut big = [0u8; 4096];
        super::fill(&mut big);
        let ones: u32 = big.iter().map(|b| b.count_ones()).sum();
        assert!((15_000..17_800).contains(&ones), "{} one bits of 32768", ones);
    }
}
