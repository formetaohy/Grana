use std::sync::{Arc, Mutex, MutexGuard};

use crate::memory::Memory;

const DISCRETE_SHARE: u64 = 4;

const INTEGRATED_SHARE: u64 = 2;

const FLOOR: u64 = 256 << 20;

const UNKNOWN: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Consumer {
    Render,
    Inference,
    Export,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Claims {
    render: u64,
    inference: u64,
    export: u64,
}

impl Claims {
    fn of(self, consumer: Consumer) -> u64 {
        match consumer {
            Consumer::Render => self.render,
            Consumer::Inference => self.inference,
            Consumer::Export => self.export,
        }
    }

    fn held(self) -> u64 {
        self.render
            .saturating_add(self.inference)
            .saturating_add(self.export)
    }
}

pub struct Ledger {
    total: u64,
    memory: Option<Memory>,
    claims: Mutex<Claims>,
}

pub struct Claim {
    ledger: Arc<Ledger>,
    consumer: Consumer,
    bytes: u64,
}

impl Ledger {
    pub fn new(total: u64) -> Arc<Self> {
        assert!(total > 0, "a ledger holds a positive device allowance");
        Arc::new(Self {
            total,
            memory: None,
            claims: Mutex::new(Claims::default()),
        })
    }

    pub fn of(memory: Option<&Memory>) -> Arc<Self> {
        let total = allowance(memory);
        Arc::new(Self {
            total,
            memory: memory.copied(),
            claims: Mutex::new(Claims::default()),
        })
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn measured(&self) -> Option<u64> {
        self.memory
            .and_then(Memory::usage)
            .map(|usage| usage.local())
    }

    pub fn claimed(&self, consumer: Consumer) -> u64 {
        self.claims().of(consumer)
    }

    pub fn spare(&self) -> u64 {
        self.total.saturating_sub(self.claims().held())
    }

    pub fn claim(self: &Arc<Self>, consumer: Consumer, bytes: u64) -> Option<Claim> {
        let mut claims = self.claims();
        if claims.held().saturating_add(bytes) > self.total {
            return None;
        }
        match consumer {
            Consumer::Render => claims.render += bytes,
            Consumer::Inference => claims.inference += bytes,
            Consumer::Export => claims.export += bytes,
        }
        Some(Claim {
            ledger: Arc::clone(self),
            consumer,
            bytes,
        })
    }

    fn claims(&self) -> MutexGuard<'_, Claims> {
        self.claims
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl Claim {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn resize(&mut self, bytes: u64) -> bool {
        if bytes == self.bytes {
            return true;
        }
        let mut claims = self.ledger.claims();
        let others = claims.held().saturating_sub(self.bytes);
        if others.saturating_add(bytes) > self.ledger.total {
            return false;
        }
        match self.consumer {
            Consumer::Render => {
                claims.render = claims.render.saturating_sub(self.bytes) + bytes;
            }
            Consumer::Inference => {
                claims.inference = claims.inference.saturating_sub(self.bytes) + bytes;
            }
            Consumer::Export => {
                claims.export = claims.export.saturating_sub(self.bytes) + bytes;
            }
        }
        self.bytes = bytes;
        true
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let mut claims = self.ledger.claims();
        match self.consumer {
            Consumer::Render => claims.render = claims.render.saturating_sub(self.bytes),
            Consumer::Inference => claims.inference = claims.inference.saturating_sub(self.bytes),
            Consumer::Export => claims.export = claims.export.saturating_sub(self.bytes),
        }
    }
}

fn allowance(memory: Option<&Memory>) -> u64 {
    let Some(memory) = memory else {
        return UNKNOWN;
    };
    let pool = memory.pool();
    let (pool, share) = if memory.integrated() {
        (
            pool.local().saturating_add(pool.non_local()),
            INTEGRATED_SHARE,
        )
    } else {
        (pool.local(), DISCRETE_SHARE)
    };
    (pool / share * (share - 1)).max(FLOOR)
}
