use grana_object::Link;

pub(super) struct Chain {
    points: Vec<usize>,
    start: u32,
    end: u32,
    incoming: u8,
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

    pub fn incoming(&self) -> u8 {
        self.incoming
    }

    pub fn outgoing(&self) -> u8 {
        self.outgoing
    }
}

pub(super) struct Chains {
    chains: Vec<Chain>,
    links: Vec<(usize, usize, Link)>,
    stamps: Vec<u64>,
    epoch: u64,
}

impl Chains {
    pub fn new() -> Self {
        Self {
            chains: Vec::new(),
            links: Vec::new(),
            stamps: Vec::new(),
            epoch: 0,
        }
    }

    pub fn push(&mut self, point: usize, frame: u32) -> usize {
        self.chains.push(Chain {
            points: vec![point],
            start: frame,
            end: frame,
            incoming: 0,
            outgoing: 0,
        });
        self.epoch += 1;
        self.stamps.push(self.epoch);
        self.chains.len() - 1
    }

    pub fn extend(&mut self, index: usize, point: usize, frame: u32) {
        let chain = &mut self.chains[index];
        assert_eq!(
            frame,
            chain.end + 1,
            "a contiguous extension advances one frame"
        );
        assert_eq!(
            chain.outgoing, 0,
            "a chain with a gap successor never extends"
        );
        chain.points.push(point);
        chain.end = frame;
        self.touch(index);
    }

    pub fn len(&self) -> usize {
        self.chains.len()
    }

    pub fn chain(&self, index: usize) -> &Chain {
        &self.chains[index]
    }

    pub fn points(&self, index: usize) -> &[usize] {
        &self.chains[index].points
    }

    pub fn links(&self) -> &[(usize, usize, Link)] {
        &self.links
    }

    pub fn stamp(&self, index: usize) -> u64 {
        self.stamps[index]
    }

    fn touch(&mut self, index: usize) {
        self.epoch += 1;
        self.stamps[index] = self.epoch;
    }

    pub fn cut(&mut self, index: usize, frame: u32) -> usize {
        let chain = &mut self.chains[index];
        let position = (frame - chain.start) as usize;
        assert!(
            position > 0 && position < chain.points.len(),
            "a cut divides a chain between two of its frames"
        );
        let points = chain.points.split_off(position);
        let end = chain.end;
        chain.end = frame - 1;
        let outgoing = chain.outgoing;
        chain.outgoing = 0;
        self.chains.push(Chain {
            points,
            start: frame,
            end,
            incoming: 0,
            outgoing,
        });
        let tail = self.chains.len() - 1;
        self.touch(index);
        self.epoch += 1;
        self.stamps.push(self.epoch);
        for link in &mut self.links {
            if link.0 == index {
                link.0 = tail;
            }
        }
        tail
    }

    pub fn attach(&mut self, parent: usize, child: usize, kind: Link) {
        assert!(
            self.chains[child].start > self.chains[parent].end,
            "a link leaves a parent before its child starts"
        );
        self.chains[parent].outgoing += 1;
        self.chains[child].incoming += 1;
        self.links.push((parent, child, kind));
        self.touch(parent);
        self.touch(child);
    }
}
