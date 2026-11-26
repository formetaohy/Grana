use glam::Vec3;

pub(super) struct Chain {
    points: Vec<usize>,
    start: u32,
    end: u32,
    velocity: Vec3,
    outgoing: u8,
}

impl Chain {
    pub fn points(&self) -> &[usize] {
        &self.points
    }

    pub fn start(&self) -> u32 {
        self.start
    }

    pub fn end(&self) -> u32 {
        self.end
    }

    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    pub fn outgoing(&self) -> u8 {
        self.outgoing
    }

    pub fn last(&self) -> usize {
        *self.points.last().expect("a chain holds a point")
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ChainLink {
    pub(super) parent: usize,
    pub(super) child: usize,
    pub(super) confidence: f64,
}

#[derive(Default)]
pub(super) struct Chains {
    chains: Vec<Chain>,
    links: Vec<ChainLink>,
}

impl Chains {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.chains.len()
    }

    pub fn chain(&self, index: usize) -> &Chain {
        &self.chains[index]
    }

    pub fn points(&self, index: usize) -> &[usize] {
        self.chains[index].points()
    }

    pub fn last(&self, index: usize) -> usize {
        self.chains[index].last()
    }

    pub fn links(&self) -> &[ChainLink] {
        &self.links
    }

    pub fn push(&mut self, point: usize, frame: u32, velocity: Vec3) -> usize {
        assert!(velocity.is_finite(), "a chain follows a finite velocity");
        self.chains.push(Chain {
            points: vec![point],
            start: frame,
            end: frame,
            velocity,
            outgoing: 0,
        });
        self.chains.len() - 1
    }

    pub fn extend(&mut self, index: usize, point: usize, frame: u32, velocity: Vec3) {
        let chain = &mut self.chains[index];
        assert_eq!(
            frame,
            chain.end + 1,
            "a contiguous extension advances one frame"
        );
        chain.points.push(point);
        chain.end = frame;
        chain.velocity = velocity;
    }

    pub fn attach(&mut self, parent: usize, child: usize, confidence: f64) {
        assert!(
            self.chains[parent].end < self.chains[child].start,
            "a link leaves a parent before its child starts"
        );
        assert!(
            self.chains[parent].outgoing < 2,
            "a chain leads to at most two chains"
        );
        assert!(
            confidence.is_finite() && confidence > 0.0 && confidence <= 1.0,
            "a solved chain link carries a probability"
        );
        self.chains[parent].outgoing += 1;
        self.links.push(ChainLink {
            parent,
            child,
            confidence,
        });
    }
}
