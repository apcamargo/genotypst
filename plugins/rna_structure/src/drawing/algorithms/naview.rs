// Port of ViennaRNA's naview.c. Keep expressions such as `(a + b) / 2.0` and
// the truncated `0.7071068` constant unchanged so coordinates remain bit-identical
// to the reference.
#![allow(
    clippy::manual_midpoint,
    clippy::approx_constant,
    clippy::unreadable_literal,
    clippy::cast_precision_loss
)]

use crate::drawing::config::NaviewOptions;
use crate::drawing::error::LayoutError;
use crate::drawing::geometry::{GEOM_EPS, Vec2, approx_eq, normalize_angle};
use crate::drawing::output::AlgorithmLayout;
use crate::drawing::validation::PairTable;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

const ANUM: f64 = 9_999.0;

#[derive(Debug, Clone)]
struct NaviewBase {
    mate: usize,
    position: Vec2,
    extracted: bool,
    region: Option<usize>, // index in regions arena
}

impl NaviewBase {
    const fn position(&self) -> Vec2 {
        self.position
    }

    fn translate(&mut self, offset: Vec2) {
        self.position.x += offset.x;
        self.position.y += offset.y;
    }
}

#[derive(Debug, Clone, Copy)]
struct Region {
    start1: usize,
    end1: usize,
    start2: usize,
    end2: usize,
}

#[derive(Debug, Clone)]
struct LoopNode {
    connections: Vec<usize>, // indices into connections arena
    depth: usize,
    mark: bool,
    radius: f64,
}

#[derive(Debug, Clone)]
struct Connection {
    loop_idx: usize,   // index of target loop
    region_idx: usize, // index of region
    start: usize,
    end: usize,
    radius: Vec2,
    angle: f64,
    extruded: bool,
    broken: bool,
}

struct NaviewContext {
    nbase: usize,
    bases: Vec<NaviewBase>,
    regions: Vec<Region>,
    loops: Vec<LoopNode>,
    connections: Vec<Connection>,
    root_idx: usize,
    lencut: f64,
}

struct NaviewTraversal {
    root: Option<usize>,
    radius: f64,
    center: Vec2,
}

#[derive(Clone, Copy)]
struct ConnectionRun {
    start: usize,
    end: usize,
    first_start: usize,
    next: usize,
    rooted: bool,
}

#[derive(Clone, Copy)]
struct ConnectionArc {
    start_radius: f64,
    end_radius: f64,
    start_angle: f64,
    delta_angle: f64,
}

impl ConnectionArc {
    fn crosses_connection_angles(
        &self,
        context: &NaviewContext,
        cp_idx: usize,
        cpnext_idx: usize,
    ) -> bool {
        let mut connection_angle =
            context.connections[cpnext_idx].angle - context.connections[cp_idx].angle;
        if connection_angle <= 0.0 {
            connection_angle += TAU;
        }
        (self.delta_angle - connection_angle).abs() > PI
    }
}

struct ConnectionScan {
    up: Option<usize>,
    down: Option<usize>,
    direction: i8,
}

impl ConnectionScan {
    const fn new(middle: usize) -> Self {
        Self {
            up: Some(middle),
            down: Some(middle),
            direction: 0,
        }
    }

    fn current(&self) -> Option<usize> {
        match self.direction.cmp(&0) {
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal => self.up,
            std::cmp::Ordering::Greater => self.down,
        }
    }

    fn advance(&mut self, start: usize, end: usize, nconn: usize) {
        if self.direction < 0 {
            self.down = self
                .down
                .and_then(|idx| (idx != end).then(|| next_connection_index(idx, nconn)));
            self.direction = 1;
        } else {
            self.up = self
                .up
                .and_then(|idx| (idx != start).then(|| previous_connection_index(idx, nconn)));
            self.direction = -1;
        }
    }

    const fn is_done(&self) -> bool {
        self.up.is_none() && self.down.is_none()
    }
}

impl NaviewContext {
    fn new(pair_table: &PairTable, lencut: f64) -> Self {
        let nbase = pair_table.len();
        let mut bases = vec![
            NaviewBase {
                mate: 0,
                position: Vec2::new(ANUM, ANUM),
                extracted: false,
                region: None,
            };
            nbase + 1
        ];
        for (index, base) in bases.iter_mut().enumerate().skip(1) {
            base.mate = pair_table.raw_partner(index);
        }

        Self {
            nbase,
            bases,
            regions: Vec::with_capacity(nbase / 2 + 2),
            loops: Vec::with_capacity(nbase / 2 + 2),
            connections: Vec::with_capacity(nbase),
            root_idx: 0,
            lencut,
        }
    }

    fn find_regions(&mut self) {
        let nb1 = self.nbase + 1;
        let mut mark = vec![false; nb1];
        self.regions.clear();

        for mut i in 0..=self.nbase {
            let mate = self.bases[i].mate;
            if mate > 0 && !mark[i] {
                let region_idx = self.regions.len();
                let mut rp = Region {
                    start1: i,
                    end1: 0,
                    start2: 0,
                    end2: mate,
                };

                mark[i] = true;
                mark[mate] = true;
                self.bases[i].region = Some(region_idx);
                self.bases[mate].region = Some(region_idx);

                i += 1;
                let mut mate_dec = mate - 1;
                while i < mate_dec && self.bases[i].mate == mate_dec {
                    mark[i] = true;
                    mark[mate_dec] = true;
                    self.bases[i].region = Some(region_idx);
                    self.bases[mate_dec].region = Some(region_idx);
                    i += 1;
                    mate_dec -= 1;
                }

                rp.end1 = i - 1;
                rp.start2 = mate_dec + 1;
                self.regions.push(rp);
            }
        }
    }

    fn construct_loop(&mut self, ibase: usize) -> Result<usize, LayoutError> {
        let loop_idx = self.loops.len();
        let node = LoopNode {
            connections: Vec::new(),
            depth: 0,
            mark: false,
            radius: 0.0,
        };
        self.loops.push(node);

        let mut i = ibase;
        loop {
            let mate = self.bases[i].mate;
            if mate != 0 {
                let rp_idx =
                    self.bases[i]
                        .region
                        .ok_or_else(|| LayoutError::InternalInvariant {
                            message: format!("missing region for base {i}"),
                        })?;
                let rp = self.regions[rp_idx];
                if !self.bases[rp.start1].extracted {
                    let mut lp_child_idx = 0;
                    if i == rp.start1 {
                        self.bases[rp.start1].extracted = true;
                        self.bases[rp.end1].extracted = true;
                        self.bases[rp.start2].extracted = true;
                        self.bases[rp.end2].extracted = true;
                        lp_child_idx = self.construct_loop(if rp.end1 < self.nbase {
                            rp.end1 + 1
                        } else {
                            0
                        })?;
                    } else if i == rp.start2 {
                        self.bases[rp.start2].extracted = true;
                        self.bases[rp.end2].extracted = true;
                        self.bases[rp.start1].extracted = true;
                        self.bases[rp.end1].extracted = true;
                        lp_child_idx = self.construct_loop(if rp.end2 < self.nbase {
                            rp.end2 + 1
                        } else {
                            0
                        })?;
                    }

                    let conn_idx = self.connections.len();
                    let (parent_start, parent_end) = if i == rp.start1 {
                        (rp.start1, rp.end2)
                    } else {
                        (rp.start2, rp.end1)
                    };
                    self.connections.push(Connection {
                        loop_idx: lp_child_idx,
                        region_idx: rp_idx,
                        start: parent_start,
                        end: parent_end,
                        radius: Vec2::zero(),
                        angle: 0.0,
                        extruded: false,
                        broken: false,
                    });

                    self.loops[loop_idx].connections.push(conn_idx);

                    let conn_symmetric_idx = self.connections.len();
                    let (child_start, child_end) = if i == rp.start1 {
                        (rp.start2, rp.end1)
                    } else {
                        (rp.start1, rp.end2)
                    };
                    self.connections.push(Connection {
                        loop_idx,
                        region_idx: rp_idx,
                        start: child_start,
                        end: child_end,
                        radius: Vec2::zero(),
                        angle: 0.0,
                        extruded: false,
                        broken: false,
                    });

                    self.loops[lp_child_idx]
                        .connections
                        .push(conn_symmetric_idx);
                }
                i = mate;
            }
            i += 1;
            if i > self.nbase {
                i = 0;
            }
            if i == ibase {
                break;
            }
        }

        Ok(loop_idx)
    }

    fn determine_depths(&mut self) {
        let count = self.loops.len();
        for i in 0..count {
            self.loops.iter_mut().for_each(|lp| lp.mark = false);
            let d = self.depth(i);
            self.loops[i].depth = usize::try_from(d).unwrap_or(0);
        }
    }

    fn depth(&mut self, lp_idx: usize) -> isize {
        let nconn = self.loops[lp_idx].connections.len();
        if nconn <= 1 {
            return 0;
        }
        if self.loops[lp_idx].mark {
            return -1;
        }
        self.loops[lp_idx].mark = true;

        let mut ret: Option<isize> = None;
        for index in 0..nconn {
            let conn_idx = self.loops[lp_idx].connections[index];
            let next_loop = self.connections[conn_idx].loop_idx;
            let depth = self.depth(next_loop);
            if depth >= 0 {
                ret = Some(ret.map_or(depth, |minimum| minimum.min(depth)));
            }
        }
        self.loops[lp_idx].mark = false;
        ret.unwrap_or(0) + 1
    }

    fn find_central_loop(&mut self) {
        self.determine_depths();
        self.root_idx = self
            .loops
            .iter()
            .enumerate()
            .max_by_key(|(idx, lp)| {
                (
                    lp.connections.len(),
                    isize::try_from(lp.depth).unwrap_or(isize::MAX),
                    std::cmp::Reverse(*idx),
                )
            })
            .map_or(0, |(idx, _)| idx);
    }

    fn determine_radius(&mut self, lp_idx: usize, lencut: f64) {
        let rt2_2 = 0.7071068;
        let nconn = self.loops[lp_idx].connections.len();
        if nconn == 0 {
            self.loops[lp_idx].radius = rt2_2;
            return;
        }

        let existing_radius = self.loops[lp_idx].radius;
        loop {
            let mut mindit = 1.0e10;
            let mut imindit = 0;
            let mut distance_sum = 0.0;
            let mut weighted_sum = 0.0;

            for i in 0..nconn {
                let cp_idx = self.loops[lp_idx].connections[i];
                let cp = &self.connections[cp_idx];

                let j = if i + 1 >= nconn { 0 } else { i + 1 };
                let cpnext_idx = self.loops[lp_idx].connections[j];
                let cpnext = &self.connections[cpnext_idx];

                let end = cp.end;
                let mut start = cpnext.start;
                if start < end {
                    start += self.nbase + 1;
                }

                let mut dt = cpnext.angle - cp.angle;
                if dt <= 0.0 {
                    dt += TAU;
                }

                let ci = if !cp.extruded {
                    (start - end) as f64
                } else if dt <= FRAC_PI_2 {
                    2.0
                } else {
                    1.5
                };

                weighted_sum += dt * (1.0 / ci + 1.0);
                distance_sum += dt * dt / ci;

                let dit = dt / ci;
                if dit < mindit && !cp.extruded && ci > 1.0 {
                    mindit = dit;
                    imindit = i;
                }
            }

            let radius = weighted_sum / distance_sum;
            let final_radius = radius.max(rt2_2);
            if mindit * final_radius < lencut {
                let to_extrude = self.loops[lp_idx].connections[imindit];
                self.connections[to_extrude].extruded = true;
            } else {
                if existing_radius <= 0.0 {
                    self.loops[lp_idx].radius = final_radius;
                }
                break;
            }
        }
    }

    fn traverse_loop(
        &mut self,
        lp_idx: usize,
        anchor_conn_idx: Option<usize>,
    ) -> Result<(), LayoutError> {
        // Placement and recursion mutate the arenas, so retain the connection
        // order in a stable snapshot for this traversal.
        let connections_list = self.loops[lp_idx].connections.clone();
        let traversal = loop {
            let mut traversal =
                self.prepare_loop_traversal(lp_idx, anchor_conn_idx, &connections_list);
            let icstart = self.first_connected_run_start(traversal.root, &connections_list);
            self.place_connected_runs(anchor_conn_idx, &connections_list, &mut traversal, icstart);
            if !self.draw_loop_backbone(&connections_list, traversal.center)? {
                break traversal;
            }
        };
        self.traverse_child_loops(&connections_list, traversal.root)?;
        Ok(())
    }

    fn prepare_loop_traversal(
        &mut self,
        lp_idx: usize,
        anchor_conn_idx: Option<usize>,
        connections_list: &[usize],
    ) -> NaviewTraversal {
        let angleinc = TAU / ((self.nbase + 1) as f64);
        let mut root = None;
        for (ic, &conn_idx) in connections_list.iter().enumerate() {
            let cp_start = self.connections[conn_idx].start;
            let cp_end = self.connections[conn_idx].end;
            let cp_region_idx = self.connections[conn_idx].region_idx;

            let xs = -((angleinc * (cp_start as f64)).sin());
            let ys = (angleinc * (cp_start as f64)).cos();
            let xe = -((angleinc * (cp_end as f64)).sin());
            let ye = (angleinc * (cp_end as f64)).cos();
            let normal = Vec2::new(ye - ys, xs - xe);
            let length = normal.length();
            let radius_vector = if length > 0.0 {
                normal / length
            } else {
                Vec2::zero()
            };

            self.connections[conn_idx].radius = radius_vector;
            self.connections[conn_idx].angle = normalize_angle(normal.y.atan2(normal.x));

            if let Some(anchor) = anchor_conn_idx
                && self.connections[anchor].region_idx == cp_region_idx
            {
                root = Some(ic);
            }
        }

        self.determine_radius(lp_idx, self.lencut);
        let radius = self.loops[lp_idx].radius;
        let center = root.map_or(Vec2::zero(), |root_idx| {
            let acp = &self.connections[connections_list[root_idx]];
            let midpoint = Vec2::new(
                (self.bases[acp.start].position.x + self.bases[acp.end].position.x) / 2.0,
                (self.bases[acp.start].position.y + self.bases[acp.end].position.y) / 2.0,
            );
            midpoint - acp.radius * radius
        });

        NaviewTraversal {
            root,
            radius,
            center,
        }
    }

    fn first_connected_run_start(
        &mut self,
        root: Option<usize>,
        connections_list: &[usize],
    ) -> usize {
        let nconn = connections_list.len();
        let mut icstart = root.unwrap_or(0);
        let mut count = 0;
        loop {
            let previous = previous_connection_index(icstart, nconn);
            let cp_previous = &self.connections[connections_list[previous]];
            let cp_current = &self.connections[connections_list[icstart]];
            if Self::connected_connection(cp_previous, cp_current) {
                icstart = previous;
            } else {
                break;
            }

            count += 1;
            if count > nconn {
                let icend = self.max_angular_gap_index(connections_list);
                self.connections[connections_list[icend]].broken = true;
                return if icend + 1 >= nconn { 0 } else { icend + 1 };
            }
        }
        icstart
    }

    fn max_angular_gap_index(&self, connections_list: &[usize]) -> usize {
        let nconn = connections_list.len();
        let mut max_angle = -1.0;
        let mut max_index = 0;
        for ic in 0..nconn {
            let next = if ic + 1 >= nconn { 0 } else { ic + 1 };
            let angle_next = self.connections[connections_list[next]].angle;
            let angle_curr = self.connections[connections_list[ic]].angle;
            let angle = normalize_angle(angle_next - angle_curr);
            if angle > max_angle {
                max_angle = angle;
                max_index = ic;
            }
        }
        max_index
    }

    fn place_connected_runs(
        &mut self,
        anchor_conn_idx: Option<usize>,
        connections_list: &[usize],
        traversal: &mut NaviewTraversal,
        mut icstart: usize,
    ) {
        let nconn = connections_list.len();
        let first_start = icstart;
        loop {
            let (icend, rooted) = self.connected_run_end(traversal.root, connections_list, icstart);
            self.place_connection_run(anchor_conn_idx, connections_list, traversal, icstart, icend);
            let icnext = next_connection_index(icend, nconn);
            self.recenter_connection_run(
                connections_list,
                traversal,
                ConnectionRun {
                    start: icstart,
                    end: icend,
                    first_start,
                    next: icnext,
                    rooted,
                },
            );
            icstart = icnext;
            if icstart == first_start {
                break;
            }
        }
    }

    fn connected_run_end(
        &self,
        root: Option<usize>,
        connections_list: &[usize],
        start: usize,
    ) -> (usize, bool) {
        let nconn = connections_list.len();
        let mut count = 0;
        let mut end = start;
        let mut rooted = false;
        loop {
            if Some(end) == root {
                rooted = true;
            }
            let next = next_connection_index(end, nconn);
            let cp = &self.connections[connections_list[end]];
            let cpnext = &self.connections[connections_list[next]];
            if Self::connected_connection(cp, cpnext) {
                count += 1;
                if count >= nconn {
                    break;
                }
                end = next;
            } else {
                break;
            }
        }
        (end, rooted)
    }

    fn place_connection_run(
        &mut self,
        anchor_conn_idx: Option<usize>,
        connections_list: &[usize],
        traversal: &NaviewTraversal,
        icstart: usize,
        icend: usize,
    ) {
        let nconn = connections_list.len();
        let middle = Self::find_ic_middle(connections_list, icstart, icend, traversal.root);
        let mut scan = ConnectionScan::new(middle);
        while !scan.is_done() {
            if let Some(ic) = scan.current() {
                let cp_idx = connections_list[ic];
                let is_anchor = traversal
                    .root
                    .is_some_and(|root| cp_idx == connections_list[root]);
                if anchor_conn_idx.is_none() || !is_anchor {
                    self.place_connection_at_direction(
                        connections_list,
                        traversal.radius,
                        traversal.center,
                        ic,
                        scan.direction,
                    );
                }
            }
            scan.advance(icstart, icend, nconn);
        }
    }

    fn place_connection_at_direction(
        &mut self,
        connections_list: &[usize],
        radius: f64,
        center: Vec2,
        ic: usize,
        direction: i8,
    ) {
        match direction.cmp(&0) {
            std::cmp::Ordering::Equal => {
                let cp = &self.connections[connections_list[ic]];
                let temp_val = 1.0 / (2.0 * radius);
                let term = if temp_val.abs() <= 1.0 {
                    temp_val.asin()
                } else {
                    0.0
                };
                let astart = cp.angle - term;
                let aend = cp.angle + term;
                self.bases[cp.start].position.x = center.x + radius * astart.cos();
                self.bases[cp.start].position.y = center.y + radius * astart.sin();
                self.bases[cp.end].position.x = center.x + radius * aend.cos();
                self.bases[cp.end].position.y = center.y + radius * aend.sin();
            }
            std::cmp::Ordering::Less => self.place_connection_before(connections_list, ic),
            std::cmp::Ordering::Greater => self.place_connection_after(connections_list, ic),
        }
    }

    fn recenter_connection_run(
        &mut self,
        connections_list: &[usize],
        traversal: &mut NaviewTraversal,
        run: ConnectionRun,
    ) {
        if run.end == run.start || (run.start == run.first_start && run.next == run.first_start) {
            return;
        }

        let cp_start = &self.connections[connections_list[run.start]];
        let cp_end = &self.connections[connections_list[run.end]];
        let start_base = cp_start.start;
        let end_base = cp_end.end;
        let delta = self.bases[end_base].position() - self.bases[start_base].position();
        let midpoint = self.bases[start_base].position() + delta / 2.0;
        let chord_length = delta.length();
        if approx_eq(chord_length, 0.0, GEOM_EPS, 0) {
            return;
        }
        let direction = delta / chord_length;

        let center_delta = traversal.center - midpoint;
        let center_direction = center_delta / chord_length;
        let dot = center_direction.dot(direction);
        let Some(normal) = (direction * dot - center_direction).normalized() else {
            return;
        };

        let start_angle = normalize_angle(
            (self.bases[start_base].position.y - traversal.center.y)
                .atan2(self.bases[start_base].position.x - traversal.center.x),
        );
        let mut end_angle = normalize_angle(
            (self.bases[end_base].position.y - traversal.center.y)
                .atan2(self.bases[end_base].position.x - traversal.center.x),
        );
        if end_angle < start_angle {
            end_angle += TAU;
        }
        let sign = if end_angle - start_angle > PI {
            -1.0
        } else {
            1.0
        };

        let adjusted_midpoint = traversal.center + normal * (sign * traversal.radius);
        let offset = adjusted_midpoint - midpoint;
        if run.rooted {
            traversal.center -= offset;
        } else {
            self.shift_connection_run(connections_list, run.start, run.end, offset);
        }
    }

    fn shift_connection_run(
        &mut self,
        connections_list: &[usize],
        start: usize,
        end: usize,
        offset: Vec2,
    ) {
        let nconn = connections_list.len();
        let mut ic = start;
        loop {
            let cp = &self.connections[connections_list[ic]];
            self.bases[cp.start].translate(offset);
            self.bases[cp.end].translate(offset);
            if ic == end {
                break;
            }
            ic = next_connection_index(ic, nconn);
        }
    }

    fn draw_loop_backbone(
        &mut self,
        connections_list: &[usize],
        center: Vec2,
    ) -> Result<bool, LayoutError> {
        let nconn = connections_list.len();
        for ic in 0..nconn {
            let next = next_connection_index(ic, nconn);
            let cp_idx = connections_list[ic];
            let cpnext_idx = connections_list[next];
            let arc = self.connection_arc(cp_idx, cpnext_idx, center);
            if arc.crosses_connection_angles(self, cp_idx, cpnext_idx)
                && !self.connections[cp_idx].extruded
                && !self.connections_are_adjacent(cp_idx, cpnext_idx)
            {
                self.connections[cp_idx].extruded = true;
                return Ok(true);
            }

            if self.connections[cp_idx].extruded {
                self.construct_extruded_segment(cp_idx, cpnext_idx)?;
            } else {
                self.construct_circular_backbone_segment(cp_idx, cpnext_idx, center, arc);
            }
        }
        Ok(false)
    }

    fn connection_arc(&self, cp_idx: usize, cpnext_idx: usize, center: Vec2) -> ConnectionArc {
        let cp_end = self.connections[cp_idx].end;
        let cpnext_start = self.connections[cpnext_idx].start;
        let start_delta = self.bases[cp_end].position() - center;
        let end_delta = self.bases[cpnext_start].position() - center;
        let mut end_angle = normalize_angle(end_delta.y.atan2(end_delta.x));
        let start_angle = normalize_angle(start_delta.y.atan2(start_delta.x));
        if end_angle < start_angle {
            end_angle += TAU;
        }

        ConnectionArc {
            start_radius: start_delta.length(),
            end_radius: end_delta.length(),
            start_angle,
            delta_angle: end_angle - start_angle,
        }
    }

    fn connections_are_adjacent(&self, cp_idx: usize, cpnext_idx: usize) -> bool {
        let cp_end = self.connections[cp_idx].end;
        let cpnext_start = self.connections[cpnext_idx].start;
        cpnext_start > cp_end && cpnext_start - cp_end == 1
    }

    fn construct_circular_backbone_segment(
        &mut self,
        cp_idx: usize,
        cpnext_idx: usize,
        center: Vec2,
        arc: ConnectionArc,
    ) {
        let start = self.connections[cp_idx].end;
        let end = self.connections[cpnext_idx].start;
        let n = connection_distance(start, end, self.nbase);
        let angleinc = arc.delta_angle / (n as f64);
        for j in 1..n {
            let i = wrapped_base_index(start + j, self.nbase);
            let angle = arc.start_angle + (j as f64) * angleinc;
            let radius = arc.start_radius
                + (arc.end_radius - arc.start_radius) * (angle - arc.start_angle) / arc.delta_angle;
            self.bases[i].position.x = center.x + radius * angle.cos();
            self.bases[i].position.y = center.y + radius * angle.sin();
        }
    }

    fn traverse_child_loops(
        &mut self,
        connections_list: &[usize],
        root: Option<usize>,
    ) -> Result<(), LayoutError> {
        for (ic, &cp_idx) in connections_list.iter().enumerate() {
            if Some(ic) != root {
                let child_loop = self.connections[cp_idx].loop_idx;
                self.generate_region(cp_idx);
                self.traverse_loop(child_loop, Some(cp_idx))?;
            }
        }
        Ok(())
    }

    fn place_connection_before(&mut self, connections_list: &[usize], ic: usize) {
        let nconn = connections_list.len();
        let next = next_connection_index(ic, nconn);
        let cp_curr = &self.connections[connections_list[ic]];
        let cpnext = &self.connections[connections_list[next]];
        let mut angle = (cp_curr.angle + cpnext.angle) / 2.0;
        if cp_curr.angle > cpnext.angle {
            angle -= PI;
        }
        let normal = Vec2::new(angle.sin(), -angle.cos());
        let delta_angle = normalize_angle(cpnext.angle - cp_curr.angle);
        let run_length = connection_run_length(cp_curr.extruded, delta_angle);
        let next_start = cpnext.start;
        let cp_end = cp_curr.end;
        self.bases[cp_end].position.x = self.bases[next_start].position.x + run_length * normal.x;
        self.bases[cp_end].position.y = self.bases[next_start].position.y + run_length * normal.y;
        self.bases[cp_curr.start].position.x = self.bases[cp_end].position.x + cp_curr.radius.y;
        self.bases[cp_curr.start].position.y = self.bases[cp_end].position.y - cp_curr.radius.x;
    }

    fn place_connection_after(&mut self, connections_list: &[usize], ic: usize) {
        let nconn = connections_list.len();
        let previous = previous_connection_index(ic, nconn);
        let cp_prev = &self.connections[connections_list[previous]];
        let cp_curr = &self.connections[connections_list[ic]];
        let curr_radius = cp_curr.radius;
        let mut angle = (cp_prev.angle + cp_curr.angle) / 2.0;
        if cp_prev.angle > cp_curr.angle {
            angle -= PI;
        }
        let normal = Vec2::new(-angle.sin(), angle.cos());
        let delta_angle = normalize_angle(cp_curr.angle - cp_prev.angle);
        let run_length = connection_run_length(cp_prev.extruded, delta_angle);
        let prev_end = cp_prev.end;
        let curr_start = cp_curr.start;
        self.bases[curr_start].position.x = self.bases[prev_end].position.x + run_length * normal.x;
        self.bases[curr_start].position.y = self.bases[prev_end].position.y + run_length * normal.y;
        self.bases[cp_curr.end].position.x = self.bases[curr_start].position.x - curr_radius.y;
        self.bases[cp_curr.end].position.y = self.bases[curr_start].position.y + curr_radius.x;
    }

    const fn connected_connection(cp: &Connection, cpnext: &Connection) -> bool {
        if cp.extruded {
            true
        } else {
            cp.end + 1 == cpnext.start
        }
    }

    fn find_ic_middle(
        connections_list: &[usize],
        icstart: usize,
        icend: usize,
        icroot: Option<usize>,
    ) -> usize {
        let nconn = connections_list.len();
        let mut count = 0;
        let mut ret = usize::MAX;
        let mut ic = icstart;
        loop {
            count += 1;
            if count > nconn * 2 {
                return ret;
            }
            if let Some(root) = icroot {
                let acp = connections_list[root];
                if connections_list[ic] == acp {
                    ret = ic;
                }
            }
            if ic == icend {
                break;
            }
            ic += 1;
            if ic >= nconn {
                ic = 0;
            }
        }
        if ret == usize::MAX {
            let mut ic_find = icstart;
            for _ in 1..count.div_ceil(2) {
                ic_find += 1;
                if ic_find >= nconn {
                    ic_find = 0;
                }
            }
            ret = ic_find;
        }
        ret
    }

    fn generate_region(&mut self, cp_idx: usize) {
        let cp = &self.connections[cp_idx];
        let rp = &self.regions[cp.region_idx];
        let (start, end) = if cp.start == rp.start1 {
            (rp.start1, rp.end1)
        } else {
            (rp.start2, rp.end2)
        };

        if self.bases[cp.start].position.x > ANUM - 100.0
            || self.bases[cp.end].position.x > ANUM - 100.0
        {
            return;
        }

        let radius = cp.radius;
        let mut l = 0.0;
        for i in (start + 1)..=end {
            l += 1.0;
            self.bases[i].position.x = self.bases[cp.start].position.x + l * radius.x;
            self.bases[i].position.y = self.bases[cp.start].position.y + l * radius.y;
            let mate = self.bases[i].mate;
            if mate > 0 {
                self.bases[mate].position.x = self.bases[cp.end].position.x + l * radius.x;
                self.bases[mate].position.y = self.bases[cp.end].position.y + l * radius.y;
            }
        }
    }

    fn construct_circle_segment(&mut self, start: usize, end: usize) {
        let mut dx = self.bases[end].position.x - self.bases[start].position.x;
        let mut dy = self.bases[end].position.y - self.bases[start].position.y;
        let rr = (dx * dx + dy * dy).sqrt();
        let l = if end >= start {
            end - start
        } else {
            end + (self.nbase + 1) - start
        };
        if rr >= (l as f64) {
            if rr > 0.0 {
                dx /= rr;
                dy /= rr;
            }
            for j in 1..l {
                let i = (start + j) % (self.nbase + 1);
                self.bases[i].position.x =
                    self.bases[start].position.x + dx * (j as f64) / (l as f64);
                self.bases[i].position.y =
                    self.bases[start].position.y + dy * (j as f64) / (l as f64);
            }
        } else {
            let (h, angleinc) = Self::find_center_for_arc(l - 1, rr);
            if approx_eq(h, 0.0, GEOM_EPS, 0) && approx_eq(angleinc, 0.0, GEOM_EPS, 0) {
                for j in 1..l {
                    let i = (start + j) % (self.nbase + 1);
                    let t = (j as f64) / (l as f64);
                    self.bases[i].position.x = self.bases[start].position.x + dx * t;
                    self.bases[i].position.y = self.bases[start].position.y + dy * t;
                }
            } else {
                if rr > 0.0 {
                    dx /= rr;
                    dy /= rr;
                }
                let midpoint_x = self.bases[start].position.x + dx * rr / 2.0;
                let midpoint_y = self.bases[start].position.y + dy * rr / 2.0;
                let normal_x = dy;
                let normal_y = -dx;
                let center_x = midpoint_x + h * normal_x;
                let center_y = midpoint_y + h * normal_y;
                let start_delta_x = self.bases[start].position.x - center_x;
                let start_delta_y = self.bases[start].position.y - center_y;
                let rr_fit = (start_delta_x * start_delta_x + start_delta_y * start_delta_y).sqrt();
                let start_angle = start_delta_y.atan2(start_delta_x);
                for j in 1..l {
                    let i = (start + j) % (self.nbase + 1);
                    let angle = start_angle + (j as f64) * angleinc;
                    self.bases[i].position.x = center_x + rr_fit * angle.cos();
                    self.bases[i].position.y = center_y + rr_fit * angle.sin();
                }
            }
        }
    }

    fn construct_extruded_segment(
        &mut self,
        cp_idx: usize,
        cpnext_idx: usize,
    ) -> Result<(), LayoutError> {
        let cp = &self.connections[cp_idx];
        let cpnext = &self.connections[cpnext_idx];
        let astart_val = cp.angle;
        let mut aend2_val = cpnext.angle;
        let aend1_val = cpnext.angle;
        if aend2_val < astart_val {
            aend2_val += TAU;
        }
        let aave_val = (astart_val + aend2_val) / 2.0;
        let mut start = cp.end;
        let mut end = cpnext.start;
        let distance = connection_distance(start, end, self.nbase);
        let mut n = isize::try_from(distance).map_err(|_| LayoutError::InternalInvariant {
            message: format!("base distance from {start} to {end} exceeds isize"),
        })?;
        let da = normalize_angle(cpnext.angle - cp.angle);

        if n == 2 {
            self.construct_circle_segment(start, end);
        } else {
            let dx = self.bases[end].position.x - self.bases[start].position.x;
            let dy = self.bases[end].position.y - self.bases[start].position.y;
            let mut rr = (dx * dx + dy * dy).sqrt();
            if rr < 1e-9 {
                rr = 1e-9;
            }
            let ndx = dx / rr;
            let ndy = dy / rr;

            if rr >= 1.5 && da <= FRAC_PI_2 {
                let nstart = wrapped_base_index(start + 1, self.nbase);
                let nend = previous_base_index_c_style(end, self.nbase);
                self.bases[nstart].position.x = self.bases[start].position.x + 0.5 * ndx;
                self.bases[nstart].position.y = self.bases[start].position.y + 0.5 * ndy;
                self.bases[nend].position.x = self.bases[end].position.x - 0.5 * ndx;
                self.bases[nend].position.y = self.bases[end].position.y - 0.5 * ndy;
                start = nstart;
                end = nend;
            }

            loop {
                let mut collision = false;
                self.construct_circle_segment(start, end);

                let nstart = wrapped_base_index(start + 1, self.nbase);
                let start_delta_x = self.bases[nstart].position.x - self.bases[start].position.x;
                let start_delta_y = self.bases[nstart].position.y - self.bases[start].position.y;
                let a1 = normalize_angle(start_delta_y.atan2(start_delta_x));
                let dac = normalize_angle(a1 - astart_val);
                if dac > PI {
                    collision = true;
                }

                let nend = previous_base_index_c_style(end, self.nbase);
                let end_delta_x = self.bases[nend].position.x - self.bases[end].position.x;
                let end_delta_y = self.bases[nend].position.y - self.bases[end].position.y;
                let a2 = normalize_angle(end_delta_y.atan2(end_delta_x));
                let dac2 = normalize_angle(aend1_val - a2);
                if dac2 > PI {
                    collision = true;
                }

                if collision {
                    let ac = aave_val.min(astart_val + 0.5);
                    self.bases[nstart].position.x = self.bases[start].position.x + ac.cos();
                    self.bases[nstart].position.y = self.bases[start].position.y + ac.sin();
                    start = nstart;

                    let ac2 = aave_val.max(aend2_val - 0.5);
                    self.bases[nend].position.x = self.bases[end].position.x + ac2.cos();
                    self.bases[nend].position.y = self.bases[end].position.y + ac2.sin();
                    end = nend;

                    n -= 2;
                }

                if !collision || n <= 1 {
                    break;
                }
            }
        }
        Ok(())
    }

    fn find_center_for_arc(segment_count: usize, chord_length: f64) -> (f64, f64) {
        let max_iterations = 500;
        let mut high_height = ((segment_count + 1) as f64) / PI;
        let mut low_height = if chord_length < 1.0 {
            0.0
        } else {
            -high_height - chord_length / ((segment_count as f64) + 1.000_001 - chord_length)
        };
        let mut iteration = 0;
        let mut candidate_height;
        let mut theta = 0.0;
        loop {
            candidate_height = (high_height + low_height) / 2.0;
            let radius =
                (candidate_height * candidate_height + chord_length * chord_length / 4.0).sqrt();
            let discriminant = 1.0 - 0.5 / (radius * radius);
            if discriminant.abs() > 1.0 {
                break;
            }
            theta = discriminant.acos();
            let phi = (candidate_height / radius).acos();
            let error = theta * ((segment_count + 1) as f64) + 2.0 * phi - TAU;
            if error > 0.0 {
                low_height = candidate_height;
            } else {
                high_height = candidate_height;
            }
            if error.abs() <= 0.0001 {
                break;
            }
            iteration += 1;
            if iteration >= max_iterations {
                break;
            }
        }
        if iteration >= max_iterations {
            candidate_height = 0.0;
            theta = 0.0;
        }
        (candidate_height, theta)
    }
}

const fn next_connection_index(index: usize, nconn: usize) -> usize {
    if index + 1 >= nconn { 0 } else { index + 1 }
}

const fn previous_connection_index(index: usize, nconn: usize) -> usize {
    if index == 0 { nconn - 1 } else { index - 1 }
}

const fn connection_run_length(extruded: bool, delta_angle: f64) -> f64 {
    if !extruded {
        1.0
    } else if delta_angle <= FRAC_PI_2 {
        2.0
    } else {
        1.5
    }
}

const fn wrapped_base_index(index: usize, nbase: usize) -> usize {
    if index > nbase {
        index - (nbase + 1)
    } else {
        index
    }
}

const fn previous_base_index_c_style(index: usize, nbase: usize) -> usize {
    if index == 0 { nbase } else { index - 1 }
}

const fn connection_distance(start: usize, end: usize, nbase: usize) -> usize {
    if end >= start {
        end - start
    } else {
        end + (nbase + 1) - start
    }
}

/// Computes RNA secondary-structure coordinates with the `NAView` algorithm.
///
/// # Errors
///
/// Returns [`LayoutError`] when the structure cannot be laid out.
pub(crate) fn layout(
    pair_table: &PairTable,
    options: &NaviewOptions,
) -> Result<AlgorithmLayout, LayoutError> {
    let n = pair_table.len();
    if !pair_table.has_pairs() {
        return Ok(AlgorithmLayout::unpaired_line(pair_table, options.scale));
    }

    let mut ctx = NaviewContext::new(pair_table, options.lencut);
    ctx.find_regions();
    ctx.construct_loop(0)?;
    ctx.find_central_loop();
    ctx.traverse_loop(ctx.root_idx, None)?;

    let coordinates: Vec<Vec2> = (1..=n)
        .map(|i| {
            let base_x = options.scale * ctx.bases[i].position.x;
            let base_y = options.scale * ctx.bases[i].position.y;
            Vec2::new(base_x, base_y)
        })
        .collect();

    let backbone =
        crate::drawing::arcs::generate_naview_backbone(pair_table, &coordinates, options.draw_arcs);

    Ok(AlgorithmLayout::new(coordinates, backbone, options.scale))
}

#[cfg(test)]
mod tests {
    use super::{
        connection_distance, layout, next_connection_index, previous_base_index_c_style,
        previous_connection_index, wrapped_base_index,
    };
    use crate::drawing::config::NaviewOptions;
    use crate::drawing::output::AlgorithmLayout;
    use crate::drawing::testing::pair_table;
    use std::f64::consts::TAU;

    fn naview(structure: &str) -> AlgorithmLayout {
        layout(&pair_table(structure), &NaviewOptions::default())
            .expect("NAVIEW layout must succeed")
    }

    fn with(structure: &str, options: &NaviewOptions) -> AlgorithmLayout {
        layout(&pair_table(structure), options).expect("NAVIEW layout must succeed")
    }

    #[test]
    fn connection_indices_wrap_around_the_ring() {
        // These are the C port's modular index helpers. An off-by-one here
        // walks a loop's connections in the wrong order.
        assert_eq!(next_connection_index(0, 4), 1);
        assert_eq!(next_connection_index(2, 4), 3);
        assert_eq!(next_connection_index(3, 4), 0);
        assert_eq!(next_connection_index(0, 1), 0);

        assert_eq!(previous_connection_index(3, 4), 2);
        assert_eq!(previous_connection_index(1, 4), 0);
        assert_eq!(previous_connection_index(0, 4), 3);
        assert_eq!(previous_connection_index(0, 1), 0);

        // The two must be inverses everywhere on the ring.
        for nconn in 1..=6 {
            for index in 0..nconn {
                assert_eq!(
                    previous_connection_index(next_connection_index(index, nconn), nconn),
                    index,
                    "nconn {nconn} index {index}"
                );
            }
        }
    }

    #[test]
    fn base_indices_wrap_using_the_c_one_based_convention() {
        // Bases are 1-based with slot 0 reserved, so the ring has `nbase + 1`
        // slots and wrapping subtracts that, not `nbase`.
        assert_eq!(wrapped_base_index(3, 5), 3);
        assert_eq!(wrapped_base_index(5, 5), 5);
        assert_eq!(wrapped_base_index(6, 5), 0);
        assert_eq!(wrapped_base_index(8, 5), 2);

        assert_eq!(previous_base_index_c_style(3, 5), 2);
        assert_eq!(previous_base_index_c_style(1, 5), 0);
        assert_eq!(previous_base_index_c_style(0, 5), 5);
    }

    #[test]
    fn connection_distance_measures_forward_around_the_ring() {
        assert_eq!(connection_distance(2, 5, 10), 3);
        assert_eq!(connection_distance(5, 5, 10), 0);
        // Wrapping past the end adds the full ring, including the reserved slot.
        assert_eq!(connection_distance(8, 2, 10), 5);
        assert_eq!(connection_distance(10, 0, 10), 1);
    }

    #[test]
    fn the_scale_option_is_a_pure_similarity_on_the_layout() {
        let structure = "(((...)))..((((....))))";
        let base = naview(structure);
        let scaled = with(
            structure,
            &NaviewOptions {
                scale: NaviewOptions::default().scale * 4.0,
                ..NaviewOptions::default()
            },
        );

        for (index, point) in base.coordinates.iter().enumerate() {
            assert!(
                (*point * 4.0).distance(scaled.coordinates[index]) < 1e-9,
                "nucleotide {index} did not scale uniformly"
            );
        }
        assert!((scaled.nominal_spacing / base.nominal_spacing - 4.0).abs() < 1e-12);
    }

    #[test]
    fn lencut_controls_whether_crowded_connections_are_extruded() {
        // `determine_radius` extrudes a connection when `mindit * radius` is
        // below `lencut`. Raising the threshold past that product must move this
        // four-way multiloop, where several connections compete.
        let structure = "(..((...))..((...))..((...))..)";
        let never = with(
            structure,
            &NaviewOptions {
                lencut: 0.0,
                ..NaviewOptions::default()
            },
        );
        let often = with(
            structure,
            &NaviewOptions {
                lencut: 2.0,
                ..NaviewOptions::default()
            },
        );
        // An extreme threshold extrudes everything it can and must still keep
        // the layout finite.
        let extreme = with(
            structure,
            &NaviewOptions {
                lencut: 100.0,
                ..NaviewOptions::default()
            },
        );

        let largest_shift = never
            .coordinates
            .iter()
            .zip(&often.coordinates)
            .map(|(left, right)| left.distance(*right))
            .fold(0.0_f64, f64::max);
        assert!(
            largest_shift > 1.0,
            "lencut must drive extrusion; largest shift was only {largest_shift}"
        );

        // Below the threshold, `lencut` is inert. Any shift above therefore
        // comes from extrusion.
        let default_cut = with(structure, &NaviewOptions::default());
        for (index, point) in never.coordinates.iter().enumerate() {
            assert!(
                point.distance(default_cut.coordinates[index]) < 1e-9,
                "nucleotide {index} moved below the extrusion threshold"
            );
        }

        for result in [&never, &often, &extreme] {
            assert_eq!(result.coordinates.len(), structure.len());
            assert!(
                result
                    .coordinates
                    .iter()
                    .all(|point| point.x.is_finite() && point.y.is_finite())
            );
        }
    }

    #[test]
    fn a_variable_radius_arc_follows_its_own_segment_winding() {
        // This segment once inherited the loop-wide turn instead of its own
        // 0.018-degree span, sending the backbone around the long side.
        let structure = "..((.(.(.(.().)())(.))..(().()).).(.())(.).((((...(()((().)(.))..)....)()..((.)())).(.)..().)(.()))())(((.)()))()";
        let result = naview(structure);
        let arc_index = 24;

        let arc = result.backbone[arc_index].expect("the regression segment must remain an arc");
        assert!(
            arc.end_radius.is_some(),
            "the regression segment must use the variable-radius path"
        );

        let start = result.coordinates[arc_index];
        let end = result.coordinates[arc_index + 1];
        let start_angle = (start - arc.center).angle();
        let end_angle = (end - arc.center).angle();
        let difference = (end_angle - start_angle).rem_euclid(TAU);
        let span = if arc.clockwise {
            difference - TAU
        } else {
            difference
        };

        assert!(
            span.abs() < 0.01,
            "the segment must follow its local arc, not the loop-wide turn, got {span}"
        );
    }
}
