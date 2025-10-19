use std::{cmp::Reverse, ops::Deref};

use keyed_priority_queue::KeyedPriorityQueue;
use log::info;
use petgraph::{graph::{node_index, DiGraph}, visit::EdgeRef, Direction};

#[derive(Clone, Debug)]
pub struct Perm {
    /// res[i] = input[ord[i]]
    pub ord: Vec<usize>,
    /// res[rank[i]] = input[i]
    pub rnk: Vec<usize>,
}

pub fn rev_perm(perm: &[usize]) -> Vec<usize> {
    let mut rev = vec![0; perm.len()];
    for (i, &p) in perm.iter().enumerate() {
        rev[p] = i;
    }
    rev
}

impl Perm {
    pub fn new_id(n: usize) -> Self {
        let ord = (0..n).collect();
        let rnk = (0..n).collect();
        Self { ord, rnk }
    }
    pub fn new_ord(ord: Vec<usize>) -> Self {
        let rnk = rev_perm(&ord);
        Self { ord, rnk }
    }
    pub fn new_rnk(rnk: Vec<usize>) -> Self {
        let ord = rev_perm(&rnk);
        Self { ord, rnk }
    }
    pub fn rev(self) -> Self {
        Self {
            ord: self.rnk.clone(),
            rnk: self.ord.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SchProblem<'g> {
    pub graph: &'g DiGraph<(), i64>,
    pub edge_weight_div: i64,
}

pub fn edge_cost(diff: usize, weight: i64) -> i64 {
    log2ceil(diff) as i64 * weight
}

impl<'g> SchProblem<'g> {
    pub fn validate(&self) {
        assert!(self.edge_weight_div > 0);
    }
    pub fn compute_cost(&self, perm: &Perm) -> f64 {
        let mut total_cost = 0;
        for (pos, &u) in perm.ord.iter().enumerate() {
            for e in self.graph.edges_directed(node_index(u), Direction::Outgoing) {
                let v = e.target().index();
                let weight = *e.weight();
                total_cost += edge_cost(perm.rnk[v] - pos, weight);
            }
            for e in self.graph.edges_directed(node_index(u), Direction::Incoming) {
                let v = e.source().index();
                let weight = *e.weight();
                total_cost += edge_cost(pos - perm.rnk[v], weight);
            }
        }
        total_cost as f64 / self.edge_weight_div as f64
    }
    pub fn print_stat(&self, perm: &Perm) {
        let init_cost = self.compute_cost(&Perm::new_id(self.graph.node_count()));
        let cost = self.compute_cost(perm);
        info!("Scheduling statistics:");
        info!("  Initial:   {:.2}", init_cost);
        info!("  Final:     {:.2}", cost);
        info!(
            "  Reduction: {:.2}%",
            (init_cost - cost) / init_cost * 100.0
        );
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LinearSchOptions {
    pub prev_factor: num::Rational64,
    pub max_iter: usize,
    pub verbose: bool,
}

impl Default for LinearSchOptions {
    fn default() -> Self {
        Self {
            prev_factor: num::Rational64::new(1, 10),
            max_iter: 10,
            verbose: true,
        }
    }
}

struct LinearScheduler<'g> {
    prob: SchProblem<'g>,
    opt: &'g LinearSchOptions,
    ord: Vec<usize>,
    rnk: Vec<usize>,
    queue: KeyedPriorityQueue<usize, Reverse<i64>>,
    prev_ord: Vec<usize>,
    prev_rnk: Vec<usize>,
    prev_cost: Vec<i64>,
}

fn log2ceil(x: usize) -> u32 {
    if x <= 1 { 0 } else { (x - 1).ilog2() + 1 }
}

impl<'g> Deref for LinearScheduler<'g> {
    type Target = SchProblem<'g>;

    fn deref(&self) -> &Self::Target {
        &self.prob
    }
}

impl<'g> LinearScheduler<'g> {
    fn new(prob: SchProblem<'g>, opt: &'g LinearSchOptions) -> Self {
        let node_count = prob.graph.node_count();
        Self {
            prob,
            opt,
            ord: (0..node_count).collect(),
            rnk: (0..node_count).collect(),
            queue: KeyedPriorityQueue::new(),
            prev_ord: (0..node_count).collect(),
            prev_rnk: (0..node_count).collect(),
            prev_cost: vec![0; node_count],
        }
    }

    fn add_node(&mut self, u: usize) {
        let pos = self.ord.len();
        let mut cost = (self.opt.prev_factor * self.prev_cost[u])
            .floor()
            .to_integer();
        for e in self.graph.edges_directed(node_index(u), Direction::Incoming) {
            let v = e.source().index();
            let w = *e.weight();
            cost += edge_cost(pos - self.rnk[v], w);
        }
        for e in self.graph.edges_directed(node_index(u), Direction::Outgoing) {
            let v = e.target().index();
            let w = *e.weight();
            cost += edge_cost(self.prev_rnk[v].max(pos + 1) - pos, w);
        }
        self.queue.push(u, Reverse(cost));
    }

    fn append_node(&mut self, u: usize) {
        let rank = self.ord.len();
        // rank_ + 1  = rank
        // 2^i <= diff_ < 2^{i+1}
        // diff_ = rank_ - pos
        // prev: diff_ = 2^{i + 1} - 1
        //       rank_ - pos = 2^{i + 1} - 1
        //       pos = rank - 2^{i + 1}
        //       pos = rank - 2^i
        //       pos >=0
        //       ==> 2^i <= rank
        //       ==> i <= rank.ilog2()
        // next: diff_ = 2^i
        //       pos - rank_ = 2^i
        //       pos = rank + 2^i - 1
        //       pos < node_count
        //       ==> rank + 2^i <= node_count
        //       ==> 2^i <= node_count - rank
        //       ==> i <= (node_count - rank).ilog2()
        if rank > 0 {
            for j in 0..=rank.ilog2() {
                let node = self.ord[rank - 2usize.pow(j)];
                for e in self.graph.edges_directed(node_index(node), Direction::Outgoing) {
                    let v = e.target().index();
                    let weight = *e.weight();
                    match self.queue.entry(v) {
                        keyed_priority_queue::Entry::Occupied(e) => {
                            let old_priority = e.get_priority().0;
                            e.set_priority(Reverse(old_priority + weight));
                        }
                        _ => {}
                    }
                }
            }
        }
        let rest = self.graph.node_count() - rank;
        for j in 0..=rest.ilog2() {
            let node = self.prev_ord[rank + 2usize.pow(j) - 1];
            for e in self.graph.edges_directed(node_index(node), Direction::Incoming) {
                let v = e.source().index();
                let weight = *e.weight();
                match self.queue.entry(v) {
                    keyed_priority_queue::Entry::Occupied(e) => {
                        let old_priority = e.get_priority().0;
                        e.set_priority(Reverse(old_priority - weight));
                    }
                    _ => {}
                }
            }
        }
        self.ord.push(u);
        self.rnk[u] = rank;
    }

    fn schedule(&mut self) {
        let mut deg = vec![0; self.graph.node_count()];
        for u in 0..self.graph.node_count() {
            deg[u] = self.graph.edges_directed(node_index(u), Direction::Incoming).count();
            if deg[u] == 0 {
                self.add_node(u);
            }
        }
        while let Some((front, _)) = self.queue.pop() {
            self.append_node(front);
            for e in self.graph.edges_directed(node_index(front), Direction::Outgoing) {
                let v = e.target().index();
                deg[v] -= 1;
                if deg[v] == 0 {
                    self.add_node(v);
                }
            }
        }
        assert_eq!(self.ord.len(), self.graph.node_count());
    }

    fn compute_force(&mut self) -> i64 {
        let mut total_cost = 0;
        for (pos, &u) in self.ord.iter().enumerate() {
            let mut force = 0;
            for e in self.graph.edges_directed(node_index(u), Direction::Outgoing) {
                let v = e.target().index();
                let w: i64 = *e.weight();
                let cost = edge_cost(self.rnk[v] - pos, w);
                force += cost;
                total_cost += cost;
            }
            for e in self.graph.edges_directed(node_index(u), Direction::Incoming) {
                let v = e.source().index();
                let w = *e.weight();
                let cost = edge_cost(pos - self.rnk[v], w);
                force -= cost;
                total_cost += cost;
            }
            self.prev_cost[u] = force;
        }
        total_cost
    }

    fn run(&mut self) {
        if self.opt.verbose {
            info!("Linear schedule with opt: {:?}", self.opt);
        }
        for it in 0..self.opt.max_iter {
            let total_cost = self.compute_force();
            if self.opt.verbose {
                info!(
                    "  It {}: cost = {}",
                    it,
                    total_cost as f32 / self.prob.edge_weight_div as f32
                );
            }
            std::mem::swap(&mut self.ord, &mut self.prev_ord);
            std::mem::swap(&mut self.rnk, &mut self.prev_rnk);
            self.ord.clear();
            self.schedule();
        }
        let total_cost = self.compute_force();
        if self.opt.verbose {
            info!(
                "  Final: cost = {}",
                total_cost as f32 / self.prob.edge_weight_div as f32
            );
        }
    }
}

pub fn linear_schedule(prob: SchProblem, opt: &LinearSchOptions) -> Perm {
    let mut scheduler = LinearScheduler::new(prob, opt);
    scheduler.run();
    Perm {
        ord: scheduler.ord,
        rnk: scheduler.rnk,
    }
}
