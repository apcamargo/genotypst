//! Fits Typst-measured RNA drawing primitives around their fixed-size content.

mod ticks {
    //! Places position ticks and labels without collisions.
    //!
    //! A tick starts at its nucleotide, extends by `offset_pt`, and carries its
    //! label at the tip. Candidates follow the outward bisector, avoid overlapping
    //! labels, then rank by local occupancy and turn from the preferred direction.
    //! Scale solving reserves every candidate footprint.

    use super::{
        Bounds, FitRequest, GEOMETRY_EPSILON, MaterializedCurve, MaterializedLine,
        MaterializedPlan, MaterializedTick, Point, PreparedTick,
    };
    use std::cmp::Ordering;
    use std::collections::HashMap;
    use std::f64::consts::{PI, TAU};

    /// Directions probed around a nucleotide, evenly spaced over a full turn.
    const DIRECTION_COUNT: usize = 12;
    /// Breathing room kept around a tick and its label.
    const CLEARANCE_PT: f64 = 1.0;
    /// Straight segments each backbone-arc cubic is flattened into.
    const ARC_FLATTEN_SEGMENTS: usize = 8;
    /// Cubic pair bows are flattened finely enough for label collision checks.
    const CURVE_FLATTEN_SEGMENTS: usize = 12;
    /// Direction magnitudes at or below this are treated as having no direction.
    const DIRECTION_EPSILON: f64 = 1e-9;
    /// Grid side length in page points. A nucleotide label spans only a few
    /// cells, keeping crowded drawings from putting every obstacle in one bucket.
    const GRID_CELL_SIZE_PT: f64 = 8.0;
    /// Maximum grid cells for an obstacle before it moves to the fallback list.
    const MAX_GRID_CELLS_PER_OBSTACLE: usize = 64;
    /// Dense-grid cell limit for page-sized RNA drawings.
    const MAX_DENSE_GRID_CELLS: usize = 16_384;

    impl Point {
        fn difference(self, other: Self) -> Self {
            Self::new(self.x - other.x, self.y - other.y)
        }

        fn dot(self, other: Self) -> f64 {
            self.x.mul_add(other.x, self.y * other.y)
        }

        fn length(self) -> f64 {
            self.dot(self).sqrt()
        }

        fn normalized(self) -> Option<Self> {
            let length = self.length();
            if length <= DIRECTION_EPSILON {
                None
            } else {
                Some(self.scaled(1.0 / length))
            }
        }

        fn perpendicular(self) -> Self {
            Self::new(-self.y, self.x)
        }
    }

    /// A line obstacle, kept with its stroke-inflated bounds for the broad phase.
    #[derive(Debug, Clone, Copy)]
    struct Segment {
        bounds: Bounds,
    }

    #[derive(Debug, Clone, Copy)]
    enum Obstacle {
        Segment(Segment),
        Rect(Bounds),
    }

    impl Obstacle {
        fn bounds(self) -> Bounds {
            match self {
                Self::Segment(segment) => segment.bounds,
                Self::Rect(bounds) => bounds,
            }
        }
    }

    /// Use contiguous buckets for page-sized drawings. Large coordinate ranges
    /// use the sparse representation instead of a mostly empty grid.
    enum GridCells {
        Dense {
            min_x: i32,
            min_y: i32,
            width: usize,
            buckets: Vec<Vec<usize>>,
        },
        Sparse(HashMap<(i32, i32), Vec<usize>>),
    }

    impl GridCells {
        fn new(bounds: Bounds) -> Self {
            let (min_x, min_y, max_x, max_y) = cell_range(bounds);
            let width = usize::try_from(i64::from(max_x) - i64::from(min_x) + 1).ok();
            let height = usize::try_from(i64::from(max_y) - i64::from(min_y) + 1).ok();
            let cell_count =
                width.and_then(|width| height.and_then(|height| width.checked_mul(height)));
            if let (Some(width), Some(cell_count)) = (width, cell_count)
                && cell_count <= MAX_DENSE_GRID_CELLS
            {
                return Self::Dense {
                    min_x,
                    min_y,
                    width,
                    buckets: (0..cell_count).map(|_| Vec::new()).collect(),
                };
            }
            Self::Sparse(HashMap::new())
        }

        fn insert(&mut self, index: usize, range: (i32, i32, i32, i32)) -> bool {
            let (min_x, min_y, max_x, max_y) = range;
            match self {
                Self::Dense {
                    min_x: grid_min_x,
                    min_y: grid_min_y,
                    width,
                    buckets,
                } => {
                    let Some(start_x) =
                        usize::try_from(i64::from(min_x) - i64::from(*grid_min_x)).ok()
                    else {
                        return false;
                    };
                    let Some(start_y) =
                        usize::try_from(i64::from(min_y) - i64::from(*grid_min_y)).ok()
                    else {
                        return false;
                    };
                    let Some(end_x) =
                        usize::try_from(i64::from(max_x) - i64::from(*grid_min_x)).ok()
                    else {
                        return false;
                    };
                    let Some(end_y) =
                        usize::try_from(i64::from(max_y) - i64::from(*grid_min_y)).ok()
                    else {
                        return false;
                    };
                    if end_x >= *width || end_y >= buckets.len() / *width {
                        return false;
                    }
                    for y in start_y..=end_y {
                        for x in start_x..=end_x {
                            buckets[y * *width + x].push(index);
                        }
                    }
                    true
                }
                Self::Sparse(buckets) => {
                    for y in min_y..=max_y {
                        for x in min_x..=max_x {
                            buckets.entry((x, y)).or_default().push(index);
                        }
                    }
                    true
                }
            }
        }

        /// Counts occupied buckets touching `bounds`, excluding the tick's owner.
        /// Each obstacle contributes once per occupied cell.
        fn occupancy(&self, bounds: Bounds, excluded: Option<usize>) -> usize {
            let (min_x, min_y, max_x, max_y) = cell_range(bounds);
            match self {
                Self::Dense {
                    min_x: grid_min_x,
                    min_y: grid_min_y,
                    width,
                    buckets,
                } => {
                    let start_x = min_x.max(*grid_min_x);
                    let start_y = min_y.max(*grid_min_y);
                    let end_x = max_x.min(*grid_min_x + *width as i32 - 1);
                    let end_y = max_y.min(*grid_min_y + (buckets.len() / *width) as i32 - 1);
                    if start_x > end_x || start_y > end_y {
                        return 0;
                    }

                    let mut occupancy: usize = 0;
                    for y in start_y..=end_y {
                        for x in start_x..=end_x {
                            let bucket = &buckets
                                [(y - *grid_min_y) as usize * *width + (x - *grid_min_x) as usize];
                            occupancy = occupancy.saturating_add(
                                bucket
                                    .iter()
                                    .filter(|&&index| Some(index) != excluded)
                                    .count(),
                            );
                        }
                    }
                    occupancy
                }
                Self::Sparse(buckets) => {
                    let mut occupancy: usize = 0;
                    for y in min_y..=max_y {
                        for x in min_x..=max_x {
                            if let Some(indices) = buckets.get(&(x, y)) {
                                occupancy = occupancy.saturating_add(
                                    indices
                                        .iter()
                                        .filter(|&&index| Some(index) != excluded)
                                        .count(),
                                );
                            }
                        }
                    }
                    occupancy
                }
            }
        }
    }

    /// Exact overlap checks for nucleotide and tick-label boxes.
    ///
    /// Unlike the density index, this tracks only text boxes, so the candidate
    /// ranker can reject unreadable labels without scanning backbone geometry.
    struct LabelIndex {
        bounds: Vec<Bounds>,
        cells: GridCells,
        oversized: Vec<usize>,
        seen: Vec<u32>,
        generation: u32,
    }

    impl LabelIndex {
        fn new(bounds: Bounds) -> Self {
            Self {
                bounds: Vec::new(),
                cells: GridCells::new(bounds),
                oversized: Vec::new(),
                seen: Vec::new(),
                generation: 0,
            }
        }

        fn insert(&mut self, bounds: Bounds) -> usize {
            let index = self.bounds.len();
            self.bounds.push(bounds);
            self.seen.push(0);

            let range = cell_range(bounds);
            let width = (i64::from(range.2) - i64::from(range.0) + 1).unsigned_abs();
            let height = (i64::from(range.3) - i64::from(range.1) + 1).unsigned_abs();
            if width.saturating_mul(height) > MAX_GRID_CELLS_PER_OBSTACLE as u64
                || !self.cells.insert(index, range)
            {
                self.oversized.push(index);
            }
            index
        }

        /// Returns whether `bounds` intersects another label box.
        fn overlaps(&mut self, bounds: Bounds, excluded: Option<usize>) -> bool {
            self.generation = self.generation.wrapping_add(1);
            if self.generation == 0 {
                self.seen.fill(0);
                self.generation = 1;
            }

            for &index in &self.oversized {
                if Some(index) != excluded && bounds_overlap(self.bounds[index], bounds) {
                    return true;
                }
            }

            let (min_x, min_y, max_x, max_y) = cell_range(bounds);
            let generation = self.generation;
            let stored_bounds = &self.bounds;
            let seen = &mut self.seen;
            match &self.cells {
                GridCells::Dense {
                    min_x: grid_min_x,
                    min_y: grid_min_y,
                    width,
                    buckets,
                } => {
                    let start_x = min_x.max(*grid_min_x);
                    let start_y = min_y.max(*grid_min_y);
                    let end_x = max_x.min(*grid_min_x + *width as i32 - 1);
                    let end_y = max_y.min(*grid_min_y + (buckets.len() / *width) as i32 - 1);
                    if start_x > end_x || start_y > end_y {
                        return false;
                    }
                    for y in start_y..=end_y {
                        for x in start_x..=end_x {
                            let bucket = &buckets
                                [(y - *grid_min_y) as usize * *width + (x - *grid_min_x) as usize];
                            if Self::bucket_overlaps(
                                bucket,
                                stored_bounds,
                                seen,
                                generation,
                                bounds,
                                excluded,
                            ) {
                                return true;
                            }
                        }
                    }
                }
                GridCells::Sparse(buckets) => {
                    for y in min_y..=max_y {
                        for x in min_x..=max_x {
                            if let Some(bucket) = buckets.get(&(x, y))
                                && Self::bucket_overlaps(
                                    bucket,
                                    stored_bounds,
                                    seen,
                                    generation,
                                    bounds,
                                    excluded,
                                )
                            {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }

        fn bucket_overlaps(
            bucket: &[usize],
            stored_bounds: &[Bounds],
            seen: &mut [u32],
            generation: u32,
            bounds: Bounds,
            excluded: Option<usize>,
        ) -> bool {
            for &index in bucket {
                if Some(index) == excluded || seen[index] == generation {
                    continue;
                }
                seen[index] = generation;
                if bounds_overlap(stored_bounds[index], bounds) {
                    return true;
                }
            }
            false
        }
    }

    /// Spatial occupancy index for tick placement.
    struct ObstacleIndex {
        bounds: Vec<Bounds>,
        cells: GridCells,
        oversized: Vec<usize>,
    }

    impl ObstacleIndex {
        fn new(bounds: Bounds) -> Self {
            Self {
                bounds: Vec::new(),
                cells: GridCells::new(bounds),
                oversized: Vec::new(),
            }
        }

        fn insert(&mut self, obstacle: Obstacle) -> usize {
            let index = self.bounds.len();
            let bounds = obstacle.bounds();
            self.bounds.push(bounds);

            let (min_x, min_y, max_x, max_y) = cell_range(bounds);
            let width = (i64::from(max_x) - i64::from(min_x) + 1).unsigned_abs();
            let height = (i64::from(max_y) - i64::from(min_y) + 1).unsigned_abs();
            let cell_count = width.saturating_mul(height);
            if cell_count > MAX_GRID_CELLS_PER_OBSTACLE as u64 {
                self.oversized.push(index);
                return index;
            }

            // `GridCells::insert` validates the whole range before touching any
            // bucket, so a rejection leaves the obstacle in no bucket at all.
            if !self.cells.insert(index, (min_x, min_y, max_x, max_y)) {
                self.oversized.push(index);
            }
            index
        }

        /// Returns a local density estimate for a candidate placement.
        fn occupancy(&self, bounds: Bounds, excluded: Option<usize>) -> usize {
            let mut occupancy = self.cells.occupancy(bounds, excluded);
            for &index in &self.oversized {
                if Some(index) != excluded && bounds_overlap(self.bounds[index], bounds) {
                    occupancy = occupancy.saturating_add(1);
                }
            }
            occupancy
        }
    }

    fn cell_range(bounds: Bounds) -> (i32, i32, i32, i32) {
        (
            cell_coordinate(bounds.min_x),
            cell_coordinate(bounds.min_y),
            cell_coordinate(bounds.max_x),
            cell_coordinate(bounds.max_y),
        )
    }

    #[allow(clippy::cast_possible_truncation)]
    fn cell_coordinate(value: f64) -> i32 {
        (value / GRID_CELL_SIZE_PT)
            .floor()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    }

    /// One placement the search may choose, expressed in page points.
    #[derive(Debug, Clone, Copy)]
    struct Candidate {
        line: Option<(Point, Point)>,
        label: Bounds,
        footprint: Bounds,
        shape: f64,
        direction_rank: usize,
    }

    /// Maximum useful clearance around a tick and its label.
    ///
    /// Beyond one nucleotide radius, `shape_cost` decides between candidates.
    fn comfort(tick: &PreparedTick) -> f64 {
        tick.offset_pt
    }

    /// How far a placement turns away from the preferred direction.
    fn shape_cost(deviation: f64) -> f64 {
        let turn = deviation / PI;
        turn * turn
    }

    /// Orders the probed directions by how far they turn from the preferred one.
    ///
    /// `bidirectional` marks a nucleotide in a collinear helix interior, where
    /// both normals are equally outward and neither should be favoured.
    #[allow(clippy::cast_precision_loss)]
    pub(super) fn direction_order(preferred: Point, bidirectional: bool) -> Vec<(Point, f64)> {
        let step = TAU / DIRECTION_COUNT as f64;
        let base = preferred.y.atan2(preferred.x);
        let mut probed: Vec<(usize, f64)> = (0..DIRECTION_COUNT)
            .map(|index| {
                let offset = index as f64 * step;
                let deviation = offset.min(TAU - offset);
                let deviation = if bidirectional {
                    deviation.min((PI - deviation).abs())
                } else {
                    deviation
                };
                (index, deviation)
            })
            .collect();
        probed.sort_by(|left, right| left.1.total_cmp(&right.1).then(left.0.cmp(&right.0)));
        probed
            .into_iter()
            .map(|(index, deviation)| {
                let (sin, cos) = (index as f64).mul_add(step, base).sin_cos();
                (Point::new(cos, sin), deviation)
            })
            .collect()
    }

    /// Resolves the direction a tick would take if nothing were in its way.
    ///
    /// The outward bisector of the two incident backbone steps points out of the
    /// local loop. It degenerates in a straight helix, where the two normals are
    /// equally outward instead.
    pub(super) fn preferred_direction(
        anchor: Point,
        previous: Option<Point>,
        next: Option<Point>,
    ) -> (Point, bool) {
        let from_previous = previous.and_then(|point| anchor.difference(point).normalized());
        let from_next = next.and_then(|point| anchor.difference(point).normalized());
        match (from_previous, from_next) {
            (Some(first), Some(second)) => first.translated(second).normalized().map_or_else(
                || (first.perpendicular(), true),
                |bisector| (bisector, false),
            ),
            (Some(only), None) | (None, Some(only)) => (only, false),
            (None, None) => (Point::new(0.0, -1.0), false),
        }
    }

    /// Visits every candidate placement, relative to `origin`.
    fn for_each_candidate(tick: &PreparedTick, origin: Point, mut visit: impl FnMut(Candidate)) {
        for (direction_rank, &(direction, deviation)) in tick.directions.iter().enumerate() {
            visit(candidate(
                tick,
                origin,
                direction,
                deviation,
                direction_rank,
            ));
        }
    }

    fn candidate(
        tick: &PreparedTick,
        origin: Point,
        direction: Point,
        deviation: f64,
        direction_rank: usize,
    ) -> Candidate {
        let length = tick.tick_len_pt;
        let line = if length > GEOMETRY_EPSILON {
            Some((
                origin.translated(direction.scaled(tick.offset_pt)),
                origin.translated(direction.scaled(tick.offset_pt + length)),
            ))
        } else {
            None
        };
        // The label-box support function measures the gap from the tick tip to
        // the box edge, not to its centre.
        let support = 0.5
            * tick
                .label_width_pt
                .mul_add(direction.x.abs(), tick.label_height_pt * direction.y.abs());
        let centre =
            origin.translated(direction.scaled(tick.offset_pt + length + tick.gap_pt + support));
        let label = Bounds::from_edges(
            centre.x - tick.label_width_pt / 2.0,
            centre.y - tick.label_height_pt / 2.0,
            centre.x + tick.label_width_pt / 2.0,
            centre.y + tick.label_height_pt / 2.0,
        );
        let footprint = line.map_or(label, |(start, end)| {
            label.include(segment_bounds(start, end, tick.half_stroke_pt))
        });
        Candidate {
            line,
            label,
            footprint,
            shape: shape_cost(deviation),
            direction_rank,
        }
    }

    /// Bounds reserved for a tick while solving the geometry scale.
    ///
    /// It contains every candidate, so the final placement cannot exceed the
    /// bounds used during solving.
    pub(super) fn reservation(tick: &PreparedTick, scale: f64) -> Bounds {
        let tick = tick.at_scale(scale);
        let mut reserved: Option<Bounds> = None;
        for_each_candidate(&tick, Point::new(0.0, 0.0), |candidate| {
            reserved = Some(reserved.map_or(candidate.footprint, |bounds: Bounds| {
                bounds.include(candidate.footprint)
            }));
        });
        reserved.unwrap_or_else(|| Bounds::from_edges(0.0, 0.0, 0.0, 0.0))
    }

    /// A tick's chosen placement, with the footprint it settled on.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct PlacedTick {
        pub(super) tick: MaterializedTick,
        /// Placement footprint relative to the scaled anchor. This makes it an
        /// affine function of the scale and lets the fit re-solve with it fixed.
        pub(super) relative: Bounds,
    }

    /// Places each tick against the materialized drawing.
    ///
    /// Ticks are added to the obstacle set in sequence, so later ticks avoid them.
    pub(super) fn place(
        request: &FitRequest,
        scale: f64,
        plan: &MaterializedPlan,
    ) -> Vec<PlacedTick> {
        let mut obstacles = ObstacleIndex::new(plan.bounds);
        let mut labels = LabelIndex::new(plan.bounds);
        // Check labels first. They are cheap to test and let score bounds skip
        // costly segment geometry in dense drawings.
        let mut box_obstacles = Vec::with_capacity(request.boxes.len());
        let mut box_labels = Vec::with_capacity(request.boxes.len());
        for (prepared, materialized) in request.boxes.iter().zip(&plan.boxes) {
            let bounds = Bounds::from_edges(
                materialized.top_left_pt.x,
                materialized.top_left_pt.y,
                materialized.top_left_pt.x + prepared.width_pt,
                materialized.top_left_pt.y + prepared.height_pt,
            );
            box_obstacles.push(obstacles.insert(Obstacle::Rect(bounds)));
            box_labels.push(labels.insert(bounds));
        }
        for (prepared, materialized) in request.lines.iter().zip(&plan.lines) {
            if let Some(line) = materialized {
                obstacles.insert(Obstacle::Segment(segment(
                    line.start_pt,
                    line.end_pt,
                    prepared.half_stroke_pt,
                )));
            }
        }
        for (prepared, materialized) in request.arcs.iter().zip(&plan.arcs) {
            if let Some(arc) = materialized {
                flatten_arc(&arc.points_pt, prepared.half_stroke_pt, &mut obstacles);
            }
        }
        for (prepared, materialized) in request.curves.iter().zip(&plan.curves) {
            if let Some(curve) = materialized {
                flatten_curve(curve, prepared.half_stroke_pt, &mut obstacles);
            }
        }

        let mut placed = Vec::with_capacity(request.ticks.len());
        for prepared in &request.ticks {
            let tick = prepared.at_scale(scale);
            let anchor = tick.anchor.scaled(scale);
            let excluded = tick.owner_box.map(|index| box_obstacles[index]);
            let excluded_label = tick.owner_box.map(|index| box_labels[index]);
            let chosen = best_candidate(
                &tick,
                anchor,
                &obstacles,
                &mut labels,
                excluded,
                excluded_label,
            );
            if let Some((start, end)) = chosen.line {
                obstacles.insert(Obstacle::Segment(segment(start, end, tick.half_stroke_pt)));
            }
            obstacles.insert(Obstacle::Rect(chosen.label));
            labels.insert(chosen.label);
            placed.push(PlacedTick {
                tick: MaterializedTick {
                    line: chosen.line.map(|(start, end)| MaterializedLine {
                        start_pt: start,
                        end_pt: end,
                    }),
                    top_left_pt: Point::new(chosen.label.min_x, chosen.label.min_y),
                },
                relative: chosen
                    .footprint
                    .translated(Point::new(-anchor.x, -anchor.y)),
            });
        }
        placed
    }

    /// Returns the lowest-density candidate.
    ///
    /// Every direction uses the same local-occupancy score, so the work is bounded
    /// by the grid cells touched by a tick footprint.
    fn best_candidate(
        tick: &PreparedTick,
        anchor: Point,
        obstacles: &ObstacleIndex,
        labels: &mut LabelIndex,
        excluded: Option<usize>,
        excluded_label: Option<usize>,
    ) -> Candidate {
        let comfort = comfort(tick);
        let mut best: Option<(bool, usize, Candidate)> = None;

        for (direction_rank, &(direction, deviation)) in tick.directions.iter().enumerate() {
            let candidate = candidate(tick, anchor, direction, deviation, direction_rank);
            let occupancy = obstacles.occupancy(
                candidate.footprint.inflated(comfort.max(CLEARANCE_PT)),
                excluded,
            );
            let label_overlaps = labels.overlaps(candidate.label, excluded_label);
            if best
                .as_ref()
                .is_none_or(|(current_overlaps, current_occupancy, current)| {
                    candidate_is_better(
                        label_overlaps,
                        occupancy,
                        candidate,
                        *current_overlaps,
                        *current_occupancy,
                        *current,
                    )
                })
            {
                best = Some((label_overlaps, occupancy, candidate));
            }
        }

        if let Some((_, _, candidate)) = best {
            candidate
        } else {
            let (direction, deviation) = tick.directions[0];
            candidate(tick, anchor, direction, deviation, 0)
        }
    }

    /// Returns whether a candidate wins the uniform local-occupancy ranking.
    fn candidate_is_better(
        label_overlaps: bool,
        occupancy: usize,
        candidate: Candidate,
        current_label_overlaps: bool,
        current_occupancy: usize,
        current: Candidate,
    ) -> bool {
        if label_overlaps != current_label_overlaps {
            return !label_overlaps;
        }
        if occupancy != current_occupancy {
            return occupancy < current_occupancy;
        }

        match candidate.shape.total_cmp(&current.shape) {
            Ordering::Less => true,
            Ordering::Equal => candidate.direction_rank < current.direction_rank,
            Ordering::Greater => false,
        }
    }

    fn segment(start: Point, end: Point, half_stroke_pt: f64) -> Segment {
        Segment {
            bounds: segment_bounds(start, end, half_stroke_pt),
        }
    }

    fn segment_bounds(start: Point, end: Point, half_stroke_pt: f64) -> Bounds {
        Bounds::from_edges(
            start.x.min(end.x),
            start.y.min(end.y),
            start.x.max(end.x),
            start.y.max(end.y),
        )
        .inflated(half_stroke_pt)
    }

    #[allow(clippy::cast_precision_loss)]
    fn flatten_arc(points: &[Point], half_stroke_pt: f64, obstacles: &mut ObstacleIndex) {
        let Some((&first, rest)) = points.split_first() else {
            return;
        };
        let mut start = first;
        for chunk in rest.chunks_exact(3) {
            let (control_1, control_2, end) = (chunk[0], chunk[1], chunk[2]);
            let mut previous = start;
            for step in 1..=ARC_FLATTEN_SEGMENTS {
                let t = step as f64 / ARC_FLATTEN_SEGMENTS as f64;
                let point = Point::new(
                    super::cubic_at(start.x, control_1.x, control_2.x, end.x, t),
                    super::cubic_at(start.y, control_1.y, control_2.y, end.y, t),
                );
                obstacles.insert(Obstacle::Segment(segment(previous, point, half_stroke_pt)));
                previous = point;
            }
            start = end;
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn flatten_curve(
        curve: &MaterializedCurve,
        half_stroke_pt: f64,
        obstacles: &mut ObstacleIndex,
    ) {
        let mut previous = curve.start_pt;
        for step in 1..=CURVE_FLATTEN_SEGMENTS {
            let t = step as f64 / CURVE_FLATTEN_SEGMENTS as f64;
            let point = Point::new(
                super::cubic_at(
                    curve.start_pt.x,
                    curve.control_1_pt.x,
                    curve.control_2_pt.x,
                    curve.end_pt.x,
                    t,
                ),
                super::cubic_at(
                    curve.start_pt.y,
                    curve.control_1_pt.y,
                    curve.control_2_pt.y,
                    curve.end_pt.y,
                    t,
                ),
            );
            obstacles.insert(Obstacle::Segment(segment(previous, point, half_stroke_pt)));
            previous = point;
        }
    }

    fn bounds_overlap(first: Bounds, second: Bounds) -> bool {
        first.min_x <= second.max_x
            && second.min_x <= first.max_x
            && first.min_y <= second.max_y
            && second.min_y <= first.max_y
    }

    #[cfg(test)]
    mod tests {
        use super::super::RawTick;
        use super::*;

        fn tick() -> PreparedTick {
            PreparedTick::new(RawTick {
                anchor: Point::new(0.0, 0.0),
                previous: Some(Point::new(-10.0, 0.0)),
                next: Some(Point::new(10.0, 0.0)),
                owner_box: None,
                offset_pt: 6.0,
                tick_len_pt: 10.0,
                resolved_connector: None,
                gap_pt: 1.0,
                half_stroke_pt: 0.5,
                label_width_pt: 12.0,
                label_height_pt: 8.0,
            })
        }

        fn obstacle_index(obstacles: &[Obstacle]) -> ObstacleIndex {
            let bounds = obstacles
                .iter()
                .map(|obstacle| obstacle.bounds())
                .reduce(Bounds::include)
                .expect("test obstacle index requires at least one obstacle");
            let mut index = ObstacleIndex::new(bounds);
            for &obstacle in obstacles {
                index.insert(obstacle);
            }
            index
        }

        fn label_index(obstacles: &[Obstacle]) -> LabelIndex {
            let bounds = obstacles
                .iter()
                .map(|obstacle| obstacle.bounds())
                .reduce(Bounds::include)
                .expect("test label index requires at least one obstacle");
            let mut index = LabelIndex::new(bounds);
            for &obstacle in obstacles {
                if let Obstacle::Rect(bounds) = obstacle {
                    index.insert(bounds);
                }
            }
            index
        }

        #[test]
        fn sparse_grid_counts_nearby_obstacles() {
            let mut index = ObstacleIndex::new(Bounds::from_edges(
                -1_000_000.0,
                -1_000_000.0,
                1_000_000.0,
                1_000_000.0,
            ));
            assert!(matches!(index.cells, GridCells::Sparse(_)));
            index.insert(Obstacle::Rect(Bounds::from_edges(10.0, 10.0, 20.0, 20.0)));

            assert_eq!(
                index.occupancy(Bounds::from_edges(12.0, 12.0, 15.0, 15.0), None),
                1
            );
        }

        #[test]
        fn positioner_uses_the_lowest_occupancy_direction() {
            let tick = tick();
            // The preferred upward normal has more nearby grid entries than the
            // diagonal at rank two.
            let obstacles = [Obstacle::Rect(Bounds::from_edges(-12.0, 6.0, 12.0, 38.0))];
            let index = obstacle_index(&obstacles);
            let mut labels = label_index(&obstacles);

            let chosen =
                best_candidate(&tick, Point::new(0.0, 0.0), &index, &mut labels, None, None);

            assert!(!labels.overlaps(chosen.label, None));
            assert_eq!(chosen.direction_rank, 2);
        }

        #[test]
        fn positioner_rejects_a_label_overlap_even_when_it_is_less_dense() {
            let tick = tick();
            let obstacles = [
                Obstacle::Rect(Bounds::from_edges(-7.0, 17.0, 7.0, 25.0)),
                Obstacle::Segment(segment(
                    Point::new(-32.0, -22.0),
                    Point::new(32.0, -22.0),
                    0.5,
                )),
            ];
            let index = obstacle_index(&obstacles);
            let mut labels = label_index(&obstacles);

            let chosen =
                best_candidate(&tick, Point::new(0.0, 0.0), &index, &mut labels, None, None);

            assert!(!labels.overlaps(chosen.label, None));
            assert_ne!(chosen.direction_rank, 0);
        }
    }
}

use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, TAU};
use thiserror::Error;

const GEOMETRY_EPSILON: f64 = 1e-9;
const FIT_TOLERANCE_PT: f64 = 0.1;
const FIT_MAX_BANDS: usize = 24;
const FIT_BINARY_STEPS: usize = 48;

#[derive(Debug, Error)]
pub(crate) enum FitError {
    #[error("invalid fit config JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("invalid fit request: {0}")]
    InvalidRequest(String),
    #[error(
        "viewport is too small for fixed-size content (current: {current_width:.4}pt × {current_height:.4}pt; required: at least {required_width:.4}pt × {required_height:.4}pt)"
    )]
    ViewportTooSmall {
        current_width: f64,
        current_height: f64,
        required_width: f64,
        required_height: f64,
    },
    #[error("serialization failed: {0}")]
    Serialization(serde_json::Error),
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WidthMode {
    Auto,
    Provisional,
    Resolved,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HeightMode {
    Auto,
    Resolved,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn scaled(self, scale: f64) -> Self {
        Self::new(self.x * scale, self.y * scale)
    }

    fn translated(self, offset: Self) -> Self {
        Self::new(self.x + offset.x, self.y + offset.y)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    geometry: Point,
    page_pt: Point,
}

impl Anchor {
    fn materialize(self, scale: f64) -> Point {
        self.geometry.scaled(scale).translated(self.page_pt)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedLine {
    start: Anchor,
    end: Anchor,
    half_stroke_pt: f64,
    min_scale: Option<f64>,
    #[serde(default)]
    minimum_visible_length_pt: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedArc {
    center: Point,
    radius: f64,
    #[serde(default)]
    end_radius: Option<f64>,
    start_angle_rad: f64,
    span_rad: f64,
    endpoint_gap_pt: f64,
    half_stroke_pt: f64,
    #[serde(default)]
    minimum_visible_length_pt: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedCurve {
    start: Anchor,
    control_1: Point,
    control_2: Point,
    end: Anchor,
    half_stroke_pt: f64,
    min_scale: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedBox {
    anchor: Point,
    width_pt: f64,
    height_pt: f64,
}

/// Connector measurements used to make a tick or terminal leader match a
/// fitted connector length.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResolvedConnector {
    span: f64,
    endpoint_gap_pt: f64,
    #[serde(default)]
    minimum_scale: Option<f64>,
    #[serde(default)]
    minimum_visible_length_pt: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedConnector {
    span: f64,
    endpoint_gap_pt: f64,
    minimum_scale: Option<f64>,
    minimum_visible_length_pt: Option<f64>,
}

impl ResolvedConnector {
    fn length_at_scale(self, scale: f64) -> f64 {
        if self
            .minimum_scale
            .is_some_and(|minimum| scale + GEOMETRY_EPSILON < minimum)
        {
            return 0.0;
        }
        let center_length = self.span * scale;
        let trimmed_length = center_length - 2.0 * self.endpoint_gap_pt;
        visible_segment_length(
            center_length,
            trimmed_length,
            self.minimum_visible_length_pt,
        )
        .unwrap_or(0.0)
    }
}

/// A position tick as Typst measures it, before a direction is chosen.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTick {
    /// Geometry position of the nucleotide the tick belongs to.
    anchor: Point,
    /// Geometry positions of its backbone neighbours, absent at a terminus.
    #[serde(default)]
    previous: Option<Point>,
    #[serde(default)]
    next: Option<Point>,
    /// Index into `boxes` of this nucleotide's own label, which the tick leaves
    /// from and must not be tested against.
    #[serde(default)]
    owner_box: Option<usize>,
    /// Distance from the nucleotide's centre to where the tick starts.
    offset_pt: f64,
    tick_len_pt: f64,
    /// When present, this tick or terminal leader uses the fitted connector
    /// length instead of `tick_len_pt`.
    #[serde(default)]
    resolved_connector: Option<RawResolvedConnector>,
    gap_pt: f64,
    half_stroke_pt: f64,
    label_width_pt: f64,
    label_height_pt: f64,
}

/// A validated tick, with the direction search's inputs derived once.
#[derive(Debug, Clone)]
struct PreparedTick {
    anchor: Point,
    owner_box: Option<usize>,
    offset_pt: f64,
    tick_len_pt: f64,
    resolved_connector: Option<ResolvedConnector>,
    gap_pt: f64,
    half_stroke_pt: f64,
    label_width_pt: f64,
    label_height_pt: f64,
    /// Probed directions, ordered by how far they turn from the preferred one,
    /// each paired with that deviation in radians.
    directions: Vec<(Point, f64)>,
}

impl PreparedTick {
    fn new(raw: RawTick) -> Self {
        let (preferred, bidirectional) =
            ticks::preferred_direction(raw.anchor, raw.previous, raw.next);
        Self {
            anchor: raw.anchor,
            owner_box: raw.owner_box,
            offset_pt: raw.offset_pt,
            tick_len_pt: raw.tick_len_pt,
            resolved_connector: raw.resolved_connector.map(|connector| ResolvedConnector {
                span: connector.span,
                endpoint_gap_pt: connector.endpoint_gap_pt,
                minimum_scale: connector.minimum_scale,
                minimum_visible_length_pt: connector.minimum_visible_length_pt,
            }),
            gap_pt: raw.gap_pt,
            half_stroke_pt: raw.half_stroke_pt,
            label_width_pt: raw.label_width_pt,
            label_height_pt: raw.label_height_pt,
            directions: ticks::direction_order(preferred, bidirectional),
        }
    }

    fn at_scale(&self, scale: f64) -> Self {
        let mut resolved = self.clone();
        if let Some(connector) = self.resolved_connector {
            resolved.tick_len_pt = connector.length_at_scale(scale);
            resolved.resolved_connector = None;
        }
        resolved
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFitRequest {
    width_mode: WidthMode,
    viewport_width_pt: Option<f64>,
    height_mode: HeightMode,
    viewport_height_pt: Option<f64>,
    target_geometry_scale: Option<f64>,
    lines: Vec<PreparedLine>,
    arcs: Vec<PreparedArc>,
    #[serde(default)]
    curves: Vec<PreparedCurve>,
    boxes: Vec<PreparedBox>,
    #[serde(default)]
    ticks: Vec<RawTick>,
}

#[derive(Debug)]
struct FitRequest {
    width_mode: WidthMode,
    viewport_width_pt: Option<f64>,
    height_mode: HeightMode,
    viewport_height_pt: Option<f64>,
    target_geometry_scale: Option<f64>,
    lines: Vec<PreparedLine>,
    arcs: Vec<PreparedArc>,
    curves: Vec<PreparedCurve>,
    boxes: Vec<PreparedBox>,
    ticks: Vec<PreparedTick>,
}

impl TryFrom<RawFitRequest> for FitRequest {
    type Error = FitError;

    fn try_from(raw: RawFitRequest) -> Result<Self, Self::Error> {
        validate_viewport(
            raw.width_mode,
            raw.viewport_width_pt,
            raw.height_mode,
            raw.viewport_height_pt,
        )?;
        if let Some(target_geometry_scale) = raw.target_geometry_scale {
            validate_positive(target_geometry_scale, "target_geometry_scale")?;
        }

        for (index, line) in raw.lines.iter().enumerate() {
            validate_point(
                line.start.geometry,
                &format!("lines[{index}].start.geometry"),
            )?;
            validate_point(line.start.page_pt, &format!("lines[{index}].start.page_pt"))?;
            validate_point(line.end.geometry, &format!("lines[{index}].end.geometry"))?;
            validate_point(line.end.page_pt, &format!("lines[{index}].end.page_pt"))?;
            validate_non_negative(
                line.half_stroke_pt,
                &format!("lines[{index}].half_stroke_pt"),
            )?;
            if let Some(min_scale) = line.min_scale {
                validate_non_negative(min_scale, &format!("lines[{index}].min_scale"))?;
            }
            if let Some(minimum_visible_length) = line.minimum_visible_length_pt {
                validate_positive(
                    minimum_visible_length,
                    &format!("lines[{index}].minimum_visible_length_pt"),
                )?;
            }
        }

        for (index, arc) in raw.arcs.iter().enumerate() {
            validate_point(arc.center, &format!("arcs[{index}].center"))?;
            validate_positive(arc.radius, &format!("arcs[{index}].radius"))?;
            if let Some(end_radius) = arc.end_radius {
                validate_positive(end_radius, &format!("arcs[{index}].end_radius"))?;
            }
            validate_finite(
                arc.start_angle_rad,
                &format!("arcs[{index}].start_angle_rad"),
            )?;
            validate_finite(arc.span_rad, &format!("arcs[{index}].span_rad"))?;
            if arc.span_rad.abs() > TAU + GEOMETRY_EPSILON {
                return Err(invalid(format!(
                    "arcs[{index}].span_rad must not exceed one full turn"
                )));
            }
            validate_non_negative(
                arc.endpoint_gap_pt,
                &format!("arcs[{index}].endpoint_gap_pt"),
            )?;
            validate_non_negative(arc.half_stroke_pt, &format!("arcs[{index}].half_stroke_pt"))?;
            if let Some(minimum_visible_length) = arc.minimum_visible_length_pt {
                validate_positive(
                    minimum_visible_length,
                    &format!("arcs[{index}].minimum_visible_length_pt"),
                )?;
            }
        }

        for (index, curve) in raw.curves.iter().enumerate() {
            validate_point(
                curve.start.geometry,
                &format!("curves[{index}].start.geometry"),
            )?;
            validate_point(
                curve.start.page_pt,
                &format!("curves[{index}].start.page_pt"),
            )?;
            validate_point(curve.control_1, &format!("curves[{index}].control_1"))?;
            validate_point(curve.control_2, &format!("curves[{index}].control_2"))?;
            validate_point(curve.end.geometry, &format!("curves[{index}].end.geometry"))?;
            validate_point(curve.end.page_pt, &format!("curves[{index}].end.page_pt"))?;
            validate_non_negative(
                curve.half_stroke_pt,
                &format!("curves[{index}].half_stroke_pt"),
            )?;
            if let Some(min_scale) = curve.min_scale {
                validate_non_negative(min_scale, &format!("curves[{index}].min_scale"))?;
            }
        }

        for (index, item) in raw.boxes.iter().enumerate() {
            validate_point(item.anchor, &format!("boxes[{index}].anchor"))?;
            validate_non_negative(item.width_pt, &format!("boxes[{index}].width_pt"))?;
            validate_non_negative(item.height_pt, &format!("boxes[{index}].height_pt"))?;
        }

        for (index, tick) in raw.ticks.iter().enumerate() {
            validate_point(tick.anchor, &format!("ticks[{index}].anchor"))?;
            if let Some(point) = tick.previous {
                validate_point(point, &format!("ticks[{index}].previous"))?;
            }
            if let Some(point) = tick.next {
                validate_point(point, &format!("ticks[{index}].next"))?;
            }
            for (value, name) in [
                (tick.offset_pt, "offset_pt"),
                (tick.tick_len_pt, "tick_len_pt"),
                (tick.gap_pt, "gap_pt"),
                (tick.half_stroke_pt, "half_stroke_pt"),
                (tick.label_width_pt, "label_width_pt"),
                (tick.label_height_pt, "label_height_pt"),
            ] {
                validate_non_negative(value, &format!("ticks[{index}].{name}"))?;
            }
            if let Some(connector) = tick.resolved_connector {
                validate_non_negative(
                    connector.span,
                    &format!("ticks[{index}].resolved_connector.span"),
                )?;
                validate_non_negative(
                    connector.endpoint_gap_pt,
                    &format!("ticks[{index}].resolved_connector.endpoint_gap_pt"),
                )?;
                if let Some(minimum_scale) = connector.minimum_scale {
                    validate_non_negative(
                        minimum_scale,
                        &format!("ticks[{index}].resolved_connector.minimum_scale"),
                    )?;
                }
                if let Some(minimum_visible_length) = connector.minimum_visible_length_pt {
                    validate_positive(
                        minimum_visible_length,
                        &format!("ticks[{index}].resolved_connector.minimum_visible_length_pt"),
                    )?;
                }
            }
            if tick.owner_box.is_some_and(|owner| owner >= raw.boxes.len()) {
                return Err(invalid(format!(
                    "ticks[{index}].owner_box must index an existing box"
                )));
            }
        }

        if raw.lines.is_empty()
            && raw.arcs.is_empty()
            && raw.curves.is_empty()
            && raw.boxes.is_empty()
        {
            return Err(invalid("at least one drawing primitive is required"));
        }

        Ok(Self {
            width_mode: raw.width_mode,
            viewport_width_pt: raw.viewport_width_pt,
            height_mode: raw.height_mode,
            viewport_height_pt: raw.viewport_height_pt,
            target_geometry_scale: raw.target_geometry_scale,
            lines: raw.lines,
            arcs: raw.arcs,
            curves: raw.curves,
            boxes: raw.boxes,
            ticks: raw.ticks.into_iter().map(PreparedTick::new).collect(),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    width: f64,
    height: f64,
}

impl Bounds {
    fn from_edges(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }

    fn from_point(point: Point) -> Self {
        Self::from_edges(point.x, point.y, point.x, point.y)
    }

    fn include(self, other: Self) -> Self {
        Self::from_edges(
            self.min_x.min(other.min_x),
            self.min_y.min(other.min_y),
            self.max_x.max(other.max_x),
            self.max_y.max(other.max_y),
        )
    }

    fn inflated(self, amount: f64) -> Self {
        Self::from_edges(
            self.min_x - amount,
            self.min_y - amount,
            self.max_x + amount,
            self.max_y + amount,
        )
    }

    fn translated(self, offset: Point) -> Self {
        Self::from_edges(
            self.min_x + offset.x,
            self.min_y + offset.y,
            self.max_x + offset.x,
            self.max_y + offset.y,
        )
    }
}

#[derive(Default)]
struct BoundsAccumulator(Option<Bounds>);

impl BoundsAccumulator {
    fn include(&mut self, bounds: Bounds) {
        self.0 = Some(self.0.map_or(bounds, |current| current.include(bounds)));
    }

    fn finish(self) -> Result<Bounds, FitError> {
        self.0
            .ok_or_else(|| invalid("no visible drawing primitives remain"))
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
struct MaterializedLine {
    start_pt: Point,
    end_pt: Point,
}

#[derive(Debug, Clone, Serialize)]
struct MaterializedArc {
    points_pt: Vec<Point>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct MaterializedCurve {
    start_pt: Point,
    control_1_pt: Point,
    control_2_pt: Point,
    end_pt: Point,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct MaterializedBox {
    top_left_pt: Point,
}

/// A placed tick: its line, absent when the tick has no length, and the top-left
/// corner of its label box.
#[derive(Debug, Clone, Copy, Serialize)]
struct MaterializedTick {
    line: Option<MaterializedLine>,
    top_left_pt: Point,
}

struct MaterializedPlan {
    lines: Vec<Option<MaterializedLine>>,
    arcs: Vec<Option<MaterializedArc>>,
    curves: Vec<Option<MaterializedCurve>>,
    boxes: Vec<MaterializedBox>,
    ticks: Vec<MaterializedTick>,
    /// Bounds of everything except the ticks, which are placed only once the
    /// geometry scale is final.
    content_bounds: Option<Bounds>,
    /// Bounds the fit works from: the content plus each tick's reservation
    /// until the ticks are placed, and their real footprints afterwards.
    bounds: Bounds,
}

impl MaterializedPlan {
    fn translated(mut self, offset: Point) -> Self {
        for line in self.lines.iter_mut().flatten() {
            line.start_pt = line.start_pt.translated(offset);
            line.end_pt = line.end_pt.translated(offset);
        }
        for arc in self.arcs.iter_mut().flatten() {
            for point in &mut arc.points_pt {
                *point = point.translated(offset);
            }
        }
        for curve in self.curves.iter_mut().flatten() {
            curve.start_pt = curve.start_pt.translated(offset);
            curve.control_1_pt = curve.control_1_pt.translated(offset);
            curve.control_2_pt = curve.control_2_pt.translated(offset);
            curve.end_pt = curve.end_pt.translated(offset);
        }
        for item in &mut self.boxes {
            item.top_left_pt = item.top_left_pt.translated(offset);
        }
        for item in &mut self.ticks {
            if let Some(line) = item.line.as_mut() {
                line.start_pt = line.start_pt.translated(offset);
                line.end_pt = line.end_pt.translated(offset);
            }
            item.top_left_pt = item.top_left_pt.translated(offset);
        }
        self.bounds = self.bounds.translated(offset);
        self
    }
}

/// Bounds the drawing occupies once its ticks are placed.
fn occupied_bounds(
    request: &FitRequest,
    scale: f64,
    plan: &MaterializedPlan,
    placed: &[ticks::PlacedTick],
) -> Result<Bounds, FitError> {
    let mut occupied = BoundsAccumulator(plan.content_bounds);
    for (tick, item) in request.ticks.iter().zip(placed) {
        occupied.include(item.relative.translated(tick.anchor.scaled(scale)));
    }
    occupied.finish()
}

#[derive(Debug, Serialize)]
struct FitResponse {
    // Typst reads only the viewport and the materialized primitives. Tests
    // also observe the solve through the fields below, which never reach Typst.
    #[cfg(test)]
    #[serde(skip)]
    width_unresolved: bool,
    #[cfg(test)]
    #[serde(skip)]
    geometry_scale: f64,
    viewport_width_pt: f64,
    viewport_height_pt: f64,
    #[cfg(test)]
    #[serde(skip)]
    translation_pt: Point,
    #[cfg(test)]
    #[serde(skip)]
    occupied_bounds_pt: Bounds,
    lines: Vec<Option<MaterializedLine>>,
    arcs: Vec<Option<MaterializedArc>>,
    curves: Vec<Option<MaterializedCurve>>,
    boxes: Vec<MaterializedBox>,
    ticks: Vec<MaterializedTick>,
}

pub(crate) fn fit(config: &[u8]) -> Result<Vec<u8>, FitError> {
    let raw: RawFitRequest = serde_json::from_slice(config)?;
    let request = FitRequest::try_from(raw)?;
    let response = fit_request(&request)?;
    serde_json::to_vec(&response).map_err(FitError::Serialization)
}

fn fit_request(request: &FitRequest) -> Result<FitResponse, FitError> {
    let width_unresolved = request.width_mode == WidthMode::Provisional;
    let width_limit = if request.width_mode == WidthMode::Resolved {
        request.viewport_width_pt
    } else {
        None
    };
    let height_limit = if request.height_mode == HeightMode::Resolved {
        request.viewport_height_pt
    } else {
        None
    };

    // Without a viewport, use `target_geometry_scale` for one nominal backbone
    // step. This gives every layout the same pitch.
    let natural_scale = width_limit.is_none() && height_limit.is_none();
    let geometry_scale = if natural_scale {
        request.target_geometry_scale.unwrap_or(1.0)
    } else {
        solve_scale(request, width_limit, height_limit)?
    };

    // The scale is fixed, so place ticks against the drawing itself.
    let mut materialized = materialize(request, geometry_scale)?;
    let placed = ticks::place(request, geometry_scale, &materialized);

    materialized.bounds = occupied_bounds(request, geometry_scale, &materialized, &placed)?;
    materialized.ticks = placed.into_iter().map(|item| item.tick).collect();

    let viewport_width = if width_unresolved {
        0.0
    } else {
        width_limit.unwrap_or(materialized.bounds.width)
    };
    let viewport_height = height_limit.unwrap_or(materialized.bounds.height);
    let translation = Point::new(
        if width_limit.is_some() {
            (viewport_width - materialized.bounds.width) / 2.0 - materialized.bounds.min_x
        } else {
            -materialized.bounds.min_x
        },
        if height_limit.is_some() {
            (viewport_height - materialized.bounds.height) / 2.0 - materialized.bounds.min_y
        } else {
            -materialized.bounds.min_y
        },
    );
    let materialized = materialized.translated(translation);

    Ok(FitResponse {
        #[cfg(test)]
        width_unresolved,
        #[cfg(test)]
        geometry_scale,
        viewport_width_pt: viewport_width,
        viewport_height_pt: viewport_height,
        #[cfg(test)]
        translation_pt: translation,
        #[cfg(test)]
        occupied_bounds_pt: materialized.bounds,
        lines: materialized.lines,
        arcs: materialized.arcs,
        curves: materialized.curves,
        boxes: materialized.boxes,
        ticks: materialized.ticks,
    })
}

fn solve_scale(
    request: &FitRequest,
    width_limit: Option<f64>,
    height_limit: Option<f64>,
) -> Result<f64, FitError> {
    let fixed_bounds = materialize(request, 0.0)?.bounds;
    if !bounds_fit(fixed_bounds, width_limit, height_limit) {
        return Err(FitError::ViewportTooSmall {
            current_width: width_limit.unwrap_or(fixed_bounds.width),
            current_height: height_limit.unwrap_or(fixed_bounds.height),
            required_width: fixed_bounds.width,
            required_height: fixed_bounds.height,
        });
    }

    let mut low = 0.0;
    let mut high = 1.0;
    let mut found_upper_bound = false;
    for _ in 0..FIT_MAX_BANDS {
        let bounds = materialize(request, high)?.bounds;
        if bounds_fit(bounds, width_limit, height_limit) {
            low = high;
            high *= 2.0;
        } else {
            found_upper_bound = true;
            break;
        }
    }

    if !found_upper_bound {
        // Scaling cannot expand the constrained span, as with a single
        // nucleotide or a flat strand fit to a height. Use the natural scale.
        return Ok(request.target_geometry_scale.unwrap_or(1.0));
    }

    for _ in 0..FIT_BINARY_STEPS {
        let mid = (low + high) / 2.0;
        let bounds = materialize(request, mid)?.bounds;
        if bounds_fit(bounds, width_limit, height_limit) {
            low = mid;
        } else {
            high = mid;
        }
    }
    Ok(low)
}

fn bounds_fit(bounds: Bounds, width_limit: Option<f64>, height_limit: Option<f64>) -> bool {
    width_limit.is_none_or(|limit| bounds.width <= limit + FIT_TOLERANCE_PT)
        && height_limit.is_none_or(|limit| bounds.height <= limit + FIT_TOLERANCE_PT)
}

fn materialize(request: &FitRequest, scale: f64) -> Result<MaterializedPlan, FitError> {
    let mut bounds = BoundsAccumulator::default();
    let mut lines = Vec::with_capacity(request.lines.len());
    let mut arcs = Vec::with_capacity(request.arcs.len());
    let mut curves = Vec::with_capacity(request.curves.len());
    let mut boxes = Vec::with_capacity(request.boxes.len());

    for primitive in &request.lines {
        let materialized = materialize_line(*primitive, scale);
        if let Some((line, line_bounds)) = materialized {
            bounds.include(line_bounds);
            lines.push(Some(line));
        } else {
            lines.push(None);
        }
    }

    for primitive in &request.arcs {
        let materialized = materialize_arc(*primitive, scale);
        if let Some((arc, arc_bounds)) = materialized {
            bounds.include(arc_bounds);
            arcs.push(Some(arc));
        } else {
            arcs.push(None);
        }
    }

    for primitive in &request.curves {
        let materialized = materialize_curve(*primitive, scale);
        if let Some((curve, curve_bounds)) = materialized {
            bounds.include(curve_bounds);
            curves.push(Some(curve));
        } else {
            curves.push(None);
        }
    }

    for primitive in &request.boxes {
        let center = primitive.anchor.scaled(scale);
        let top_left = Point::new(
            center.x - primitive.width_pt / 2.0,
            center.y - primitive.height_pt / 2.0,
        );
        let box_bounds = Bounds::from_edges(
            top_left.x,
            top_left.y,
            top_left.x + primitive.width_pt,
            top_left.y + primitive.height_pt,
        );
        bounds.include(box_bounds);
        boxes.push(MaterializedBox {
            top_left_pt: top_left,
        });
    }

    // A tick's direction depends on scale. While solving, reserve the union of
    // its candidate placements. Connector-derived lengths resolve at this scale,
    // so the reservation covers both fixed ticks and leaders.
    let content_bounds = bounds.0;
    for tick in &request.ticks {
        bounds.include(ticks::reservation(tick, scale).translated(tick.anchor.scaled(scale)));
    }

    Ok(MaterializedPlan {
        lines,
        arcs,
        curves,
        boxes,
        ticks: Vec::new(),
        content_bounds,
        bounds: bounds.finish()?,
    })
}

fn visible_segment_length(
    center_length: f64,
    trimmed_length: f64,
    minimum_visible_length: Option<f64>,
) -> Option<f64> {
    match minimum_visible_length {
        None => (trimmed_length > GEOMETRY_EPSILON).then_some(trimmed_length),
        Some(minimum) if center_length + GEOMETRY_EPSILON < minimum => None,
        Some(minimum) => Some(trimmed_length.max(minimum)),
    }
}

fn backbone_visible_length(
    path_length: f64,
    endpoint_gap: f64,
    minimum_visible_length: Option<f64>,
) -> Option<f64> {
    let visible_length = path_length - 2.0 * endpoint_gap;
    if visible_length <= GEOMETRY_EPSILON {
        return None;
    }
    if minimum_visible_length.is_some_and(|minimum| visible_length + GEOMETRY_EPSILON < minimum) {
        return None;
    }
    Some(visible_length)
}

fn materialize_line(primitive: PreparedLine, scale: f64) -> Option<(MaterializedLine, Bounds)> {
    if primitive
        .min_scale
        .is_some_and(|minimum| scale + GEOMETRY_EPSILON < minimum)
    {
        return None;
    }

    let start = primitive.start.materialize(scale);
    let end = primitive.end.materialize(scale);
    let center_start = primitive.start.geometry.scaled(scale);
    let center_end = primitive.end.geometry.scaled(scale);
    let center_dx = center_end.x - center_start.x;
    let center_dy = center_end.y - center_start.y;
    let center_length = center_dx.hypot(center_dy);
    if center_length <= GEOMETRY_EPSILON {
        return None;
    }
    let unit_x = center_dx / center_length;
    let unit_y = center_dy / center_length;
    let trimmed_length = (end.x - start.x) * unit_x + (end.y - start.y) * unit_y;
    let endpoint_gap = (center_length - trimmed_length) / 2.0;
    backbone_visible_length(
        center_length,
        endpoint_gap,
        primitive.minimum_visible_length_pt,
    )?;

    let line = MaterializedLine {
        start_pt: start,
        end_pt: end,
    };
    let line_bounds = Bounds::from_edges(
        start.x.min(end.x),
        start.y.min(end.y),
        start.x.max(end.x),
        start.y.max(end.y),
    )
    .inflated(primitive.half_stroke_pt);
    Some((line, line_bounds))
}

fn materialize_curve(primitive: PreparedCurve, scale: f64) -> Option<(MaterializedCurve, Bounds)> {
    if primitive
        .min_scale
        .is_some_and(|minimum| scale + GEOMETRY_EPSILON < minimum)
    {
        return None;
    }

    let curve = MaterializedCurve {
        start_pt: primitive.start.materialize(scale),
        control_1_pt: primitive.control_1.scaled(scale),
        control_2_pt: primitive.control_2.scaled(scale),
        end_pt: primitive.end.materialize(scale),
    };
    let curve_bounds = cubic_segment_bounds(
        curve.start_pt,
        curve.control_1_pt,
        curve.control_2_pt,
        curve.end_pt,
    )
    .inflated(primitive.half_stroke_pt);
    Some((curve, curve_bounds))
}

fn materialize_arc(primitive: PreparedArc, scale: f64) -> Option<(MaterializedArc, Bounds)> {
    let radius_pt = primitive.radius * scale;
    if radius_pt <= GEOMETRY_EPSILON {
        return None;
    }

    let center = primitive.center.scaled(scale);
    let points = if let Some(end_radius) = primitive.end_radius {
        let end_radius_pt = end_radius * scale;
        if end_radius_pt <= GEOMETRY_EPSILON {
            return None;
        }
        let unit_length =
            polar_arc_length(primitive.radius, end_radius, primitive.span_rad, 0.0, 1.0);
        let arc_length = unit_length * scale;
        backbone_visible_length(
            arc_length,
            primitive.endpoint_gap_pt,
            primitive.minimum_visible_length_pt,
        )?;
        let endpoint_gap = primitive.endpoint_gap_pt / scale;
        let start_t = polar_arc_parameter_at_length(
            primitive.radius,
            end_radius,
            primitive.span_rad,
            endpoint_gap,
            unit_length,
        );
        let end_t = polar_arc_parameter_at_length(
            primitive.radius,
            end_radius,
            primitive.span_rad,
            unit_length - endpoint_gap,
            unit_length,
        );
        bezier_polar_arc_points(
            center,
            primitive.start_angle_rad,
            primitive.span_rad,
            radius_pt,
            end_radius_pt,
            start_t,
            end_t,
        )
    } else {
        let arc_length = primitive.span_rad.abs() * radius_pt;
        backbone_visible_length(
            arc_length,
            primitive.endpoint_gap_pt,
            primitive.minimum_visible_length_pt,
        )?;
        let endpoint_gap = primitive.endpoint_gap_pt;

        let direction = primitive.span_rad.signum();
        let gap_angle = endpoint_gap / radius_pt;
        let start_angle = primitive.start_angle_rad + direction * gap_angle;
        let span = primitive.span_rad - direction * 2.0 * gap_angle;
        bezier_arc_points(center, start_angle, span, radius_pt)
    };
    let bounds = cubic_path_bounds(&points)?.inflated(primitive.half_stroke_pt);
    Some((MaterializedArc { points_pt: points }, bounds))
}

fn polar_arc_length(
    start_radius: f64,
    end_radius: f64,
    span: f64,
    start_t: f64,
    end_t: f64,
) -> f64 {
    let radius_delta = end_radius - start_radius;
    let parameter_span = end_t - start_t;
    if parameter_span == 0.0 {
        return 0.0;
    }

    let sweep = span.abs();
    if sweep == 0.0 {
        return radius_delta.abs() * parameter_span;
    }

    let maximum_radius = start_radius.abs().max(end_radius.abs()).max(1.0);
    if radius_delta.abs() <= f64::EPSILON * maximum_radius {
        return sweep * start_radius * parameter_span;
    }

    let antiderivative = |radius: f64| {
        let speed = radius_delta.hypot(sweep * radius);
        0.5 * (radius * speed
            + radius_delta * radius_delta / sweep * (sweep * radius / radius_delta.abs()).asinh())
    };
    let start_radius = start_radius + radius_delta * start_t;
    let end_radius = start_radius + radius_delta * parameter_span;
    (antiderivative(end_radius) - antiderivative(start_radius)) / radius_delta
}

fn polar_arc_parameter_at_length(
    start_radius: f64,
    end_radius: f64,
    span: f64,
    target_length: f64,
    total_length: f64,
) -> f64 {
    if target_length <= 0.0 {
        return 0.0;
    }
    if target_length >= total_length {
        return 1.0;
    }

    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..48 {
        let mid = (low + high) / 2.0;
        let length = polar_arc_length(start_radius, end_radius, span, 0.0, mid);
        if length < target_length {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) / 2.0
}

fn polar_arc_point(
    center: Point,
    start_angle: f64,
    span: f64,
    start_radius: f64,
    end_radius: f64,
    t: f64,
) -> Point {
    let radius = start_radius + (end_radius - start_radius) * t;
    let angle = start_angle + span * t;
    Point::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

fn polar_arc_derivative(
    start_angle: f64,
    span: f64,
    start_radius: f64,
    end_radius: f64,
    t: f64,
) -> Point {
    let radius_delta = end_radius - start_radius;
    let radius = start_radius + radius_delta * t;
    let angle = start_angle + span * t;
    Point::new(
        radius_delta * angle.cos() - radius * span * angle.sin(),
        radius_delta * angle.sin() + radius * span * angle.cos(),
    )
}

fn bezier_polar_arc_points(
    center: Point,
    start_angle: f64,
    span: f64,
    start_radius: f64,
    end_radius: f64,
    start_t: f64,
    end_t: f64,
) -> Vec<Point> {
    let segment_count = ((span * (end_t - start_t)).abs() / FRAC_PI_2).ceil() as usize;
    let segment_count = segment_count.max(1);
    let step = (end_t - start_t) / segment_count as f64;
    let mut points = Vec::with_capacity(1 + 3 * segment_count);

    for segment in 0..segment_count {
        let segment_start = start_t + step * segment as f64;
        let segment_end = segment_start + step;
        let start = polar_arc_point(
            center,
            start_angle,
            span,
            start_radius,
            end_radius,
            segment_start,
        );
        let end = polar_arc_point(
            center,
            start_angle,
            span,
            start_radius,
            end_radius,
            segment_end,
        );
        let start_derivative =
            polar_arc_derivative(start_angle, span, start_radius, end_radius, segment_start);
        let end_derivative =
            polar_arc_derivative(start_angle, span, start_radius, end_radius, segment_end);
        if segment == 0 {
            points.push(start);
        }
        points.push(Point::new(
            start.x + start_derivative.x * step / 3.0,
            start.y + start_derivative.y * step / 3.0,
        ));
        points.push(Point::new(
            end.x - end_derivative.x * step / 3.0,
            end.y - end_derivative.y * step / 3.0,
        ));
        points.push(end);
    }
    points
}

fn bezier_arc_points(center: Point, angle: f64, span: f64, radius: f64) -> Vec<Point> {
    let segment_count = ((span.abs() / FRAC_PI_2).ceil() as usize).max(1);
    let segment_span = span / segment_count as f64;
    let y0 = (segment_span / 2.0).sin();
    let x0 = (segment_span / 2.0).cos();
    let k = 4.0 / 3.0 * (segment_span / 4.0).tan();
    let px = [x0, x0 + k * y0, x0 + k * y0, x0];
    let py = [-y0, -y0 + k * x0, y0 - k * x0, y0];
    let mut points = Vec::with_capacity(1 + 3 * segment_count);

    for segment in 0..segment_count {
        let start = angle + segment_span * segment as f64;
        let (sin, cos) = (start + segment_span / 2.0).sin_cos();
        for index in 0..4 {
            if segment > 0 && index == 0 {
                continue;
            }
            points.push(Point::new(
                center.x + radius * (px[index] * cos - py[index] * sin),
                center.y + radius * (px[index] * sin + py[index] * cos),
            ));
        }
    }
    points
}

fn cubic_path_bounds(points: &[Point]) -> Option<Bounds> {
    let (&first, rest) = points.split_first()?;
    let mut bounds = Bounds::from_point(first);
    let mut start = first;
    for chunk in rest.chunks_exact(3) {
        let control_1 = chunk[0];
        let control_2 = chunk[1];
        let end = chunk[2];
        bounds = bounds.include(cubic_segment_bounds(start, control_1, control_2, end));
        start = end;
    }
    Some(bounds)
}

fn cubic_segment_bounds(start: Point, control_1: Point, control_2: Point, end: Point) -> Bounds {
    let mut bounds = Bounds::from_point(start).include(Bounds::from_point(end));
    for t in cubic_extrema(start.x, control_1.x, control_2.x, end.x) {
        let point = Point::new(
            cubic_at(start.x, control_1.x, control_2.x, end.x, t),
            cubic_at(start.y, control_1.y, control_2.y, end.y, t),
        );
        bounds = bounds.include(Bounds::from_point(point));
    }
    for t in cubic_extrema(start.y, control_1.y, control_2.y, end.y) {
        let point = Point::new(
            cubic_at(start.x, control_1.x, control_2.x, end.x, t),
            cubic_at(start.y, control_1.y, control_2.y, end.y, t),
        );
        bounds = bounds.include(Bounds::from_point(point));
    }
    bounds
}

fn cubic_extrema(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 3.0 * p0 - 6.0 * p1 + 3.0 * p2;
    let c = -3.0 * p0 + 3.0 * p1;
    let qa = 3.0 * a;
    let qb = 2.0 * b;
    let mut roots = Vec::with_capacity(2);

    if qa.abs() <= GEOMETRY_EPSILON {
        if qb.abs() > GEOMETRY_EPSILON {
            let t = -c / qb;
            if t > 0.0 && t < 1.0 {
                roots.push(t);
            }
        }
        return roots;
    }

    let discriminant = qb * qb - 4.0 * qa * c;
    if discriminant < 0.0 {
        return roots;
    }
    let root = discriminant.sqrt();
    for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
        if t > 0.0 && t < 1.0 {
            roots.push(t);
        }
    }
    roots
}

fn cubic_at(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let one_minus_t = 1.0 - t;
    one_minus_t.powi(3) * p0
        + 3.0 * one_minus_t.powi(2) * t * p1
        + 3.0 * one_minus_t * t.powi(2) * p2
        + t.powi(3) * p3
}

fn validate_viewport(
    width_mode: WidthMode,
    width: Option<f64>,
    height_mode: HeightMode,
    height: Option<f64>,
) -> Result<(), FitError> {
    match (width_mode, width) {
        (WidthMode::Resolved, Some(value)) => validate_positive(value, "viewport_width_pt")?,
        (WidthMode::Resolved, None) => {
            return Err(invalid(
                "viewport_width_pt is required when width_mode is resolved",
            ));
        }
        (WidthMode::Auto | WidthMode::Provisional, None) => {}
        (WidthMode::Auto | WidthMode::Provisional, Some(_)) => {
            return Err(invalid(
                "viewport_width_pt must be omitted unless width_mode is resolved",
            ));
        }
    }
    match (height_mode, height) {
        (HeightMode::Resolved, Some(value)) => validate_positive(value, "viewport_height_pt")?,
        (HeightMode::Resolved, None) => {
            return Err(invalid(
                "viewport_height_pt is required when height_mode is resolved",
            ));
        }
        (HeightMode::Auto, None) => {}
        (HeightMode::Auto, Some(_)) => {
            return Err(invalid(
                "viewport_height_pt must be omitted when height_mode is auto",
            ));
        }
    }
    Ok(())
}

fn validate_point(point: Point, name: &str) -> Result<(), FitError> {
    validate_finite(point.x, &format!("{name}.x"))?;
    validate_finite(point.y, &format!("{name}.y"))
}

fn validate_positive(value: f64, name: &str) -> Result<(), FitError> {
    validate_finite(value, name)?;
    if value <= 0.0 {
        return Err(invalid(format!("{name} must be positive")));
    }
    Ok(())
}

fn validate_non_negative(value: f64, name: &str) -> Result<(), FitError> {
    validate_finite(value, name)?;
    if value < 0.0 {
        return Err(invalid(format!("{name} must be non-negative")));
    }
    Ok(())
}

fn validate_finite(value: f64, name: &str) -> Result<(), FitError> {
    if !value.is_finite() {
        return Err(invalid(format!("{name} must be finite")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> FitError {
    FitError::InvalidRequest(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(x: f64, y: f64) -> Anchor {
        Anchor {
            geometry: Point::new(x, y),
            page_pt: Point::new(0.0, 0.0),
        }
    }

    fn request(width: Option<f64>, height: Option<f64>) -> FitRequest {
        FitRequest {
            width_mode: if width.is_some() {
                WidthMode::Resolved
            } else {
                WidthMode::Auto
            },
            viewport_width_pt: width,
            height_mode: if height.is_some() {
                HeightMode::Resolved
            } else {
                HeightMode::Auto
            },
            viewport_height_pt: height,
            target_geometry_scale: None,
            lines: vec![PreparedLine {
                start: anchor(0.0, 0.0),
                end: anchor(10.0, 0.0),
                half_stroke_pt: 1.0,
                min_scale: None,
                minimum_visible_length_pt: None,
            }],
            arcs: Vec::new(),
            boxes: vec![
                PreparedBox {
                    anchor: Point::new(0.0, 0.0),
                    width_pt: 4.0,
                    height_pt: 6.0,
                },
                PreparedBox {
                    anchor: Point::new(10.0, 0.0),
                    width_pt: 4.0,
                    height_pt: 6.0,
                },
            ],
            curves: Vec::new(),
            ticks: Vec::new(),
        }
    }

    fn short_backbone_line() -> PreparedLine {
        PreparedLine {
            start: Anchor {
                geometry: Point::new(0.0, 0.0),
                page_pt: Point::new(2.5, 0.0),
            },
            end: Anchor {
                geometry: Point::new(0.5, 0.0),
                page_pt: Point::new(-2.5, 0.0),
            },
            half_stroke_pt: 0.5,
            min_scale: None,
            minimum_visible_length_pt: Some(2.0),
        }
    }

    #[test]
    fn visible_length_tolerance_keeps_a_boundary_connector() {
        let minimum = 2.0 + GEOMETRY_EPSILON;

        assert_eq!(backbone_visible_length(2.0, 0.0, Some(minimum)), Some(2.0));
    }

    fn tick(anchor: Point, previous: Point, next: Point) -> PreparedTick {
        PreparedTick::new(RawTick {
            anchor,
            previous: Some(previous),
            next: Some(next),
            owner_box: None,
            offset_pt: 6.0,
            tick_len_pt: 10.0,
            resolved_connector: None,
            gap_pt: 1.0,
            half_stroke_pt: 0.5,
            label_width_pt: 12.0,
            label_height_pt: 8.0,
        })
    }

    fn label_bounds(prepared: &PreparedTick, placed: MaterializedTick) -> Bounds {
        Bounds::from_edges(
            placed.top_left_pt.x,
            placed.top_left_pt.y,
            placed.top_left_pt.x + prepared.label_width_pt,
            placed.top_left_pt.y + prepared.label_height_pt,
        )
    }

    fn overlaps(first: Bounds, second: Bounds) -> bool {
        first.min_x < second.max_x
            && second.min_x < first.max_x
            && first.min_y < second.max_y
            && second.min_y < first.max_y
    }

    fn assert_contained(response: &FitResponse) {
        assert!(response.occupied_bounds_pt.min_x >= -FIT_TOLERANCE_PT);
        assert!(response.occupied_bounds_pt.min_y >= -FIT_TOLERANCE_PT);
        assert!(response.occupied_bounds_pt.max_x <= response.viewport_width_pt + FIT_TOLERANCE_PT);
        assert!(
            response.occupied_bounds_pt.max_y <= response.viewport_height_pt + FIT_TOLERANCE_PT
        );
    }

    /// Builds a fit request from a real layout, as `render-rna-structure` does. It creates
    /// one line per backbone step and one box per nucleotide in normalized units.
    fn request_from_layout(structure: &str, algorithm: &str) -> FitRequest {
        let config = format!(r#"{{"algorithm":"{algorithm}"}}"#);
        let parsed =
            crate::drawing::config::parse(config.as_bytes()).expect("layout config must parse");
        let input = crate::drawing::validation::validate(&"A".repeat(structure.len()), structure)
            .expect("structure must validate");
        let layout = crate::drawing::algorithms::layout(&parsed, &input.pair_table)
            .expect("layout must succeed");

        // Match `into_response`: divide by the layout's nominal step so one
        // geometry unit means one step for every algorithm.
        let divisor = layout.nominal_spacing;
        assert!(divisor.is_finite() && divisor > 0.0);
        let coordinates: Vec<Point> = layout
            .coordinates
            .iter()
            .map(|point| Point::new(point.x / divisor, -point.y / divisor))
            .collect();

        let lines = coordinates
            .windows(2)
            .map(|step| PreparedLine {
                start: Anchor {
                    geometry: step[0],
                    page_pt: Point::new(0.0, 0.0),
                },
                end: Anchor {
                    geometry: step[1],
                    page_pt: Point::new(0.0, 0.0),
                },
                half_stroke_pt: 0.5,
                min_scale: None,
                minimum_visible_length_pt: None,
            })
            .collect();
        let boxes = coordinates
            .iter()
            .map(|&anchor| PreparedBox {
                anchor,
                width_pt: 6.0,
                height_pt: 6.0,
            })
            .collect();

        FitRequest {
            width_mode: WidthMode::Auto,
            viewport_width_pt: None,
            height_mode: HeightMode::Auto,
            viewport_height_pt: None,
            target_geometry_scale: None,
            lines,
            arcs: Vec::new(),
            curves: Vec::new(),
            boxes,
            ticks: Vec::new(),
        }
    }

    fn median_line_length(response: &FitResponse) -> f64 {
        let mut lengths: Vec<f64> = response
            .lines
            .iter()
            .flatten()
            .map(|line| (line.end_pt.x - line.start_pt.x).hypot(line.end_pt.y - line.start_pt.y))
            .collect();
        assert!(!lengths.is_empty(), "the fit must materialize some lines");
        lengths.sort_by(f64::total_cmp);
        lengths[lengths.len() / 2]
    }

    /// A tick on a straight run of three nucleotides, with an obstacle box one
    /// geometry unit away on the side the tick prefers.
    fn blocked_tick_request(target_scale: f64) -> FitRequest {
        let mut request = request(None, None);
        request.target_geometry_scale = Some(target_scale);
        request.lines.clear();
        request.boxes = vec![
            PreparedBox {
                anchor: Point::new(0.0, 0.0),
                width_pt: 6.0,
                height_pt: 8.0,
            },
            PreparedBox {
                anchor: Point::new(0.0, 1.0),
                width_pt: 26.0,
                height_pt: 26.0,
            },
        ];
        request.ticks = vec![PreparedTick::new(RawTick {
            anchor: Point::new(0.0, 0.0),
            previous: Some(Point::new(-1.0, 0.0)),
            next: Some(Point::new(1.0, 0.0)),
            owner_box: Some(0),
            offset_pt: 6.0,
            tick_len_pt: 10.0,
            resolved_connector: None,
            gap_pt: 1.0,
            half_stroke_pt: 0.5,
            label_width_pt: 12.0,
            label_height_pt: 8.0,
        })];
        request
    }

    /// Smallest gap between a tick's label and any of the request's boxes other
    /// than the nucleotide the tick belongs to.
    fn label_clearance(request: &FitRequest, response: &FitResponse, index: usize) -> f64 {
        let prepared = &request.ticks[index];
        let label = label_bounds(prepared, response.ticks[index]);
        request
            .boxes
            .iter()
            .zip(&response.boxes)
            .enumerate()
            .filter(|(box_index, _)| Some(*box_index) != prepared.owner_box)
            .map(|(_, (prepared_box, materialized))| {
                let bounds = Bounds::from_edges(
                    materialized.top_left_pt.x,
                    materialized.top_left_pt.y,
                    materialized.top_left_pt.x + prepared_box.width_pt,
                    materialized.top_left_pt.y + prepared_box.height_pt,
                );
                let dx = (bounds.min_x - label.max_x)
                    .max(label.min_x - bounds.max_x)
                    .max(0.0);
                let dy = (bounds.min_y - label.max_y)
                    .max(label.min_y - bounds.max_y)
                    .max(0.0);
                dx.hypot(dy)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Viewport negotiation: natural size, the three width modes, height
    /// derivation, slack centring, and scale invariance.
    mod sizing {
        use super::*;

        #[test]
        fn natural_size_includes_text_and_stroke_extents() {
            let response = fit_request(&request(None, None)).expect("natural fit should succeed");
            assert!((response.viewport_width_pt - 14.0).abs() < GEOMETRY_EPSILON);
            assert!((response.viewport_height_pt - 6.0).abs() < GEOMETRY_EPSILON);
            assert!((response.occupied_bounds_pt.min_x).abs() < GEOMETRY_EPSILON);
            assert!((response.occupied_bounds_pt.max_x - 14.0).abs() < GEOMETRY_EPSILON);
        }

        #[test]
        fn provisional_width_uses_genotypst_zero_width_contract() {
            let mut request = request(None, None);
            request.width_mode = WidthMode::Provisional;

            let response = fit_request(&request).expect("provisional fit should succeed");

            assert!(response.width_unresolved);
            assert_eq!(response.geometry_scale, 1.0);
            assert_eq!(response.viewport_width_pt, 0.0);
            assert!((response.occupied_bounds_pt.width - 14.0).abs() < GEOMETRY_EPSILON);
        }

        #[test]
        fn resolved_width_uses_maximum_scale_and_derives_height() {
            let response =
                fit_request(&request(Some(24.0), None)).expect("resolved fit should succeed");
            assert!((response.geometry_scale - 2.0).abs() <= FIT_TOLERANCE_PT);
            assert!((response.viewport_width_pt - 24.0).abs() < GEOMETRY_EPSILON);
            assert!((response.viewport_height_pt - 6.0).abs() < GEOMETRY_EPSILON);
            assert_contained(&response);
            assert!(
                (response.occupied_bounds_pt.width - response.viewport_width_pt).abs()
                    <= FIT_TOLERANCE_PT
            );
        }

        #[test]
        fn auto_width_with_resolved_height_fills_height_and_derives_width() {
            let mut request = request(None, Some(24.0));
            request.lines[0].end.geometry = Point::new(0.0, 10.0);
            request.boxes[1].anchor = Point::new(0.0, 10.0);

            let response = fit_request(&request).expect("height-constrained fit should succeed");

            assert!((response.geometry_scale - 1.8).abs() <= FIT_TOLERANCE_PT);
            assert!((response.viewport_width_pt - 4.0).abs() <= FIT_TOLERANCE_PT);
            assert!((response.viewport_height_pt - 24.0).abs() < GEOMETRY_EPSILON);
            assert_contained(&response);
            assert!(
                (response.occupied_bounds_pt.height - response.viewport_height_pt).abs()
                    <= FIT_TOLERANCE_PT
            );
        }

        #[test]
        fn both_dimensions_center_slack_on_non_limiting_axis() {
            let response = fit_request(&request(Some(24.0), Some(20.0)))
                .expect("two-dimensional fit should succeed");
            assert!((response.occupied_bounds_pt.min_x).abs() <= FIT_TOLERANCE_PT);
            assert!((response.occupied_bounds_pt.min_y - 7.0).abs() <= FIT_TOLERANCE_PT);
            assert!((response.occupied_bounds_pt.max_y - 13.0).abs() <= FIT_TOLERANCE_PT);
        }

        #[test]
        fn viewport_smaller_than_fixed_boxes_is_rejected() {
            let error = fit_request(&request(Some(3.0), Some(5.0)))
                .expect_err("fixed content should not fit");
            assert!(matches!(error, FitError::ViewportTooSmall { .. }));

            // Typst shows this message verbatim. Include both the requested and
            // required dimensions so the user can choose a workable viewport.
            let message = error.to_string();
            assert!(
                message.contains("viewport is too small for fixed-size content"),
                "{message}"
            );
            assert!(
                message.contains("current: 3.0000pt × 5.0000pt"),
                "{message}"
            );
            assert!(message.contains("required: at least"), "{message}");

            let FitError::ViewportTooSmall {
                required_width,
                required_height,
                ..
            } = error
            else {
                panic!("expected a viewport error");
            };
            // The reported minimum must itself fit.
            assert!(
                fit_request(&request(Some(required_width), Some(required_height))).is_ok(),
                "the reported minimum viewport must itself fit"
            );
        }

        #[test]
        fn one_nucleotide_pitch_is_the_same_under_every_layout() {
            // Every layout reports nominal backbone steps, so an intrinsic fit should
            // draw the same connector for every algorithm. This depends on
            // normalization and the fitter.
            let structure = "(((((.......)))))...((((....))))..";
            let mut pitches = Vec::new();

            for algorithm in ["radial", "naview", "rna_turtle", "rna_puzzler", "circular"] {
                let response = fit_request(&request_from_layout(structure, algorithm))
                    .unwrap_or_else(|error| panic!("{algorithm} must fit: {error}"));
                pitches.push((algorithm, median_line_length(&response)));
            }

            let (_, reference) = pitches[0];
            assert!(reference > 0.0, "the reference pitch must be positive");
            for &(algorithm, pitch) in &pitches {
                assert!(
                    (pitch - reference).abs() < 0.01,
                    "{algorithm} drew a {pitch}pt median connector, \
                     but {} drew {reference}pt",
                    pitches[0].0
                );
            }
        }

        #[test]
        fn fixed_size_content_does_not_change_with_the_viewport() {
            // Nucleotide circles and glyphs stay fixed in document units. Only the
            // geometry may rescale.
            let structure = "(((((.......)))))...((((....))))..";
            let mut small = request_from_layout(structure, "naview");
            small.width_mode = WidthMode::Resolved;
            small.viewport_width_pt = Some(200.0);
            let mut large = request_from_layout(structure, "naview");
            large.width_mode = WidthMode::Resolved;
            large.viewport_width_pt = Some(600.0);

            let small_fit = fit_request(&small).expect("small viewport must fit");
            let large_fit = fit_request(&large).expect("large viewport must fit");

            assert!(
                large_fit.geometry_scale > small_fit.geometry_scale * 2.0,
                "the geometry must actually rescale between the two viewports"
            );
            // Each box stays centred on its scaled anchor at its own fixed size,
            // so its top-left sits half that size from the anchor at any scale.
            for (request, fit) in [(&small, &small_fit), (&large, &large_fit)] {
                for (prepared, placed) in request.boxes.iter().zip(&fit.boxes) {
                    let center = prepared.anchor.scaled(fit.geometry_scale);
                    let expected_x = center.x - prepared.width_pt / 2.0 + fit.translation_pt.x;
                    let expected_y = center.y - prepared.height_pt / 2.0 + fit.translation_pt.y;
                    assert!((placed.top_left_pt.x - expected_x).abs() < 1e-9);
                    assert!((placed.top_left_pt.y - expected_y).abs() < 1e-9);
                }
                assert_contained(fit);
            }
        }

        #[test]
        fn resolved_fit_is_invariant_under_coordinate_rescaling() {
            // Normalization divides every coordinate by a constant. A viewport fit
            // must absorb that in `geometry_scale` without moving the points.
            const K: f64 = 25.0;

            let baseline = FitRequest {
                width_mode: WidthMode::Resolved,
                viewport_width_pt: Some(120.0),
                height_mode: HeightMode::Auto,
                viewport_height_pt: None,
                target_geometry_scale: None,
                lines: vec![PreparedLine {
                    start: anchor(0.0, 0.0),
                    end: anchor(10.0, 4.0),
                    half_stroke_pt: 0.5,
                    min_scale: Some(0.25),
                    minimum_visible_length_pt: None,
                }],
                arcs: vec![PreparedArc {
                    center: Point::new(4.0, 2.0),
                    radius: 6.0,
                    end_radius: None,
                    start_angle_rad: 0.3,
                    span_rad: FRAC_PI_2,
                    endpoint_gap_pt: 1.0,
                    half_stroke_pt: 0.5,
                    minimum_visible_length_pt: None,
                }],
                curves: Vec::new(),
                boxes: vec![
                    PreparedBox {
                        anchor: Point::new(0.0, 0.0),
                        width_pt: 5.0,
                        height_pt: 7.0,
                    },
                    PreparedBox {
                        anchor: Point::new(10.0, 4.0),
                        width_pt: 5.0,
                        height_pt: 7.0,
                    },
                ],
                ticks: vec![tick(
                    Point::new(5.0, 2.0),
                    Point::new(0.0, 0.0),
                    Point::new(10.0, 4.0),
                )],
            };

            let mut rescaled = FitRequest {
                lines: baseline.lines.clone(),
                arcs: baseline.arcs.clone(),
                curves: baseline.curves.clone(),
                boxes: baseline.boxes.clone(),
                ticks: vec![tick(
                    Point::new(5.0 / K, 2.0 / K),
                    Point::new(0.0, 0.0),
                    Point::new(10.0 / K, 4.0 / K),
                )],
                ..baseline
            };
            for line in &mut rescaled.lines {
                line.start.geometry = line.start.geometry.scaled(1.0 / K);
                line.end.geometry = line.end.geometry.scaled(1.0 / K);
                line.min_scale = line.min_scale.map(|minimum| minimum * K);
            }
            for arc in &mut rescaled.arcs {
                arc.center = arc.center.scaled(1.0 / K);
                arc.radius /= K;
            }
            for item in &mut rescaled.boxes {
                item.anchor = item.anchor.scaled(1.0 / K);
            }

            let baseline = fit_request(&baseline).expect("baseline fit should succeed");
            let rescaled = fit_request(&rescaled).expect("rescaled fit should succeed");

            assert!((rescaled.geometry_scale - baseline.geometry_scale * K).abs() < 1e-9);
            assert!((rescaled.viewport_width_pt - baseline.viewport_width_pt).abs() < 1e-9);
            assert!((rescaled.viewport_height_pt - baseline.viewport_height_pt).abs() < 1e-9);
            for (rescaled, baseline) in rescaled.boxes.iter().zip(&baseline.boxes) {
                assert!((rescaled.top_left_pt.x - baseline.top_left_pt.x).abs() < 1e-9);
                assert!((rescaled.top_left_pt.y - baseline.top_left_pt.y).abs() < 1e-9);
            }
            for (rescaled, baseline) in rescaled.ticks.iter().zip(&baseline.ticks) {
                assert!((rescaled.top_left_pt.x - baseline.top_left_pt.x).abs() < 1e-9);
                assert!((rescaled.top_left_pt.y - baseline.top_left_pt.y).abs() < 1e-9);
            }
        }

        #[test]
        fn scale_invariant_single_nucleotide_keeps_natural_scale_and_centers() {
            let request = FitRequest {
                width_mode: WidthMode::Resolved,
                viewport_width_pt: Some(40.0),
                height_mode: HeightMode::Resolved,
                viewport_height_pt: Some(30.0),
                target_geometry_scale: None,
                lines: Vec::new(),
                arcs: Vec::new(),
                curves: Vec::new(),
                boxes: vec![PreparedBox {
                    anchor: Point::new(0.0, 0.0),
                    width_pt: 8.0,
                    height_pt: 10.0,
                }],
                ticks: Vec::new(),
            };

            let response = fit_request(&request).expect("single nucleotide should fit");

            assert_eq!(response.geometry_scale, 1.0);
            assert!((response.occupied_bounds_pt.min_x - 16.0).abs() < GEOMETRY_EPSILON);
            assert!((response.occupied_bounds_pt.min_y - 10.0).abs() < GEOMETRY_EPSILON);
            assert_contained(&response);
        }

        #[test]
        fn flat_strand_fit_to_height_keeps_target_scale() {
            // A height limit cannot constrain a strand with no vertical extent, so
            // the fit must fall back to the requested pitch rather than 1pt per step.
            let box_at = |x: f64| PreparedBox {
                anchor: Point::new(x, 0.0),
                width_pt: 8.0,
                height_pt: 10.0,
            };
            let line_between = |from: f64, to: f64| PreparedLine {
                start: anchor(from, 0.0),
                end: anchor(to, 0.0),
                half_stroke_pt: 0.5,
                min_scale: None,
                minimum_visible_length_pt: Some(2.0),
            };
            let request = FitRequest {
                width_mode: WidthMode::Auto,
                viewport_width_pt: None,
                height_mode: HeightMode::Resolved,
                viewport_height_pt: Some(80.0),
                target_geometry_scale: Some(12.0),
                lines: vec![line_between(0.0, 1.0), line_between(1.0, 2.0)],
                arcs: Vec::new(),
                curves: Vec::new(),
                boxes: vec![box_at(0.0), box_at(1.0), box_at(2.0)],
                ticks: Vec::new(),
            };

            let response = fit_request(&request).expect("flat strand should fit");

            assert_eq!(response.geometry_scale, 12.0);
            assert!(response.lines.iter().all(Option::is_some));
            assert_contained(&response);
        }
    }

    /// Straight connectors: when one is drawn, how long it is drawn, and the
    /// separate `min_scale` visibility switch.
    mod line_materialization {
        use super::*;

        /// Visibility and length of one connector across the whole scale range.
        ///
        /// `short_backbone_line` spans half a geometry unit, has a 2pt visible
        /// minimum, and reserves 2.5pt of endpoint clearance at each end. Its drawn
        /// length is therefore `0.5 * scale - 5`, which reaches the 2pt minimum at
        /// scale 14. Below that the connector is dropped rather than drawn too
        /// short, and at the threshold it is widened to exactly the minimum.
        #[test]
        fn line_visibility_and_length_across_scales() {
            // Check endpoints only where full clearance is expected. The NaN
            // rows opt out of that check.
            let drawn = [
                // Exactly at the threshold: widened up to the 2pt minimum.
                (14.0, 2.0, f64::NAN, f64::NAN),
                // Above it: full clearance preserved at both ends.
                (16.0, 3.0, 2.5, 5.5),
                // Well above it: purely proportional.
                (18.0, 4.0, f64::NAN, f64::NAN),
            ];

            for (scale, expected_length, start_x, end_x) in drawn {
                let (line, _) = materialize_line(short_backbone_line(), scale)
                    .unwrap_or_else(|| panic!("scale {scale} must draw the connector"));
                let length =
                    (line.end_pt.x - line.start_pt.x).hypot(line.end_pt.y - line.start_pt.y);
                assert!(
                    (length - expected_length).abs() < GEOMETRY_EPSILON,
                    "scale {scale}: drew {length}, expected {expected_length}"
                );
                if !start_x.is_nan() {
                    assert!(
                        (line.start_pt.x - start_x).abs() < GEOMETRY_EPSILON,
                        "scale {scale}: start {} != {start_x}",
                        line.start_pt.x
                    );
                    assert!(
                        (line.end_pt.x - end_x).abs() < GEOMETRY_EPSILON,
                        "scale {scale}: end {} != {end_x}",
                        line.end_pt.x
                    );
                }
            }

            for (scale, reason) in [
                (8.0, "no room for both endpoint gaps"),
                (13.9, "below the minimum visible length"),
            ] {
                assert!(
                    materialize_line(short_backbone_line(), scale).is_none(),
                    "scale {scale}: must be omitted, {reason}"
                );
            }

            // `min_scale` is a separate mechanism: it hides the connector outright
            // rather than shortening it, and it is applied by `materialize`.
            let mut request = request(None, None);
            request.lines[0].min_scale = Some(2.0);
            let plan = materialize(&request, 1.0).expect("boxes keep the plan visible");
            assert!(plan.lines[0].is_none(), "min_scale must hide the connector");
        }

        #[test]
        fn resolved_connector_length_tracks_scale_and_visibility_rules() {
            let unconstrained = ResolvedConnector {
                span: 10.0,
                endpoint_gap_pt: 3.0,
                minimum_scale: None,
                minimum_visible_length_pt: None,
            };
            assert!((unconstrained.length_at_scale(1.0) - 4.0).abs() < GEOMETRY_EPSILON);
            assert!((unconstrained.length_at_scale(2.0) - 14.0).abs() < GEOMETRY_EPSILON);

            let floored = ResolvedConnector {
                span: 3.0,
                endpoint_gap_pt: 2.0,
                minimum_scale: None,
                minimum_visible_length_pt: Some(2.0),
            };
            assert!((floored.length_at_scale(1.0) - 2.0).abs() < GEOMETRY_EPSILON);

            let omitted = ResolvedConnector {
                span: 1.0,
                endpoint_gap_pt: 2.0,
                minimum_scale: None,
                minimum_visible_length_pt: Some(2.0),
            };
            assert_eq!(omitted.length_at_scale(1.0), 0.0);

            let hidden = ResolvedConnector {
                span: 10.0,
                endpoint_gap_pt: 3.0,
                minimum_scale: Some(2.0),
                minimum_visible_length_pt: None,
            };
            assert_eq!(hidden.length_at_scale(1.0), 0.0);
        }

        #[test]
        fn connector_derived_tick_uses_the_nominal_length_at_each_scale() {
            let prepared = PreparedTick::new(RawTick {
                anchor: Point::new(0.0, 0.0),
                previous: Some(Point::new(-1.0, 0.0)),
                next: Some(Point::new(1.0, 0.0)),
                owner_box: None,
                offset_pt: 6.0,
                tick_len_pt: 0.0,
                resolved_connector: Some(RawResolvedConnector {
                    span: 10.0,
                    endpoint_gap_pt: 3.0,
                    minimum_scale: None,
                    minimum_visible_length_pt: None,
                }),
                gap_pt: 1.0,
                half_stroke_pt: 0.5,
                label_width_pt: 12.0,
                label_height_pt: 8.0,
            });

            let natural = prepared.at_scale(1.0);
            let constrained = prepared.at_scale(0.8);

            assert!(natural.resolved_connector.is_none());
            assert!(constrained.resolved_connector.is_none());
            assert!((natural.tick_len_pt - 4.0).abs() < GEOMETRY_EPSILON);
            assert!((constrained.tick_len_pt - 2.0).abs() < GEOMETRY_EPSILON);
        }
    }

    /// Curved connectors: constant-radius arcs, tapered variable-radius arcs,
    /// cubic curves, and the bounds they report.
    mod arc_materialization {
        use super::*;

        /// The same half-turn arc at two scales. At scale 2 the drawn span cannot
        /// carry both endpoint gaps and the 2pt minimum, so it is dropped. At
        /// scale 3 it fits and both ends are trimmed by exactly the gap.
        #[test]
        fn constant_radius_arc_visibility_across_scales() {
            let primitive = || PreparedArc {
                center: Point::new(0.0, 0.0),
                radius: 1.0,
                end_radius: None,
                start_angle_rad: 0.0,
                span_rad: std::f64::consts::PI,
                endpoint_gap_pt: 3.0,
                half_stroke_pt: 0.5,
                minimum_visible_length_pt: Some(2.0),
            };

            assert!(
                materialize_arc(primitive(), 2.0).is_none(),
                "an arc without room for both gaps must be omitted"
            );

            let (arc, _) = materialize_arc(primitive(), 3.0)
                .expect("a connector with room for both gaps should materialize");
            let first = arc.points_pt.first().expect("arc needs a start point");
            let last = arc.points_pt.last().expect("arc needs an end point");

            assert!((first.x - 3.0 * 1.0f64.cos()).abs() < GEOMETRY_EPSILON);
            assert!((first.y - 3.0 * 1.0f64.sin()).abs() < GEOMETRY_EPSILON);
            assert!((last.x + 3.0 * 1.0f64.cos()).abs() < GEOMETRY_EPSILON);
            assert!((last.y - 3.0 * 1.0f64.sin()).abs() < GEOMETRY_EPSILON);
        }

        #[test]
        fn variable_radius_arc_preserves_polar_endpoints_and_splits_long_spans() {
            let primitive = PreparedArc {
                center: Point::new(0.0, 0.0),
                radius: 10.0,
                end_radius: Some(14.0),
                start_angle_rad: 0.0,
                span_rad: std::f64::consts::PI,
                endpoint_gap_pt: 0.0,
                half_stroke_pt: 0.5,
                minimum_visible_length_pt: None,
            };
            let (arc, bounds) =
                materialize_arc(primitive, 1.0).expect("a variable-radius arc should materialize");
            let first = arc.points_pt.first().expect("arc needs a start point");
            let last = arc.points_pt.last().expect("arc needs an end point");

            assert!((first.x - 10.0).abs() < GEOMETRY_EPSILON);
            assert!(first.y.abs() < GEOMETRY_EPSILON);
            assert!((last.x + 14.0).abs() < GEOMETRY_EPSILON);
            assert!(last.y.abs() < GEOMETRY_EPSILON);
            assert_eq!(arc.points_pt.len(), 7, "a half turn uses two cubics");
            assert!(bounds.min_x <= -14.5);
            assert!(bounds.max_x >= 10.5);
        }

        #[test]
        fn trimmed_variable_radius_arc_removes_equal_curve_length_from_each_end() {
            let primitive = PreparedArc {
                center: Point::new(0.0, 0.0),
                radius: 10.0,
                end_radius: Some(14.0),
                start_angle_rad: 0.0,
                span_rad: FRAC_PI_2,
                endpoint_gap_pt: 2.0,
                half_stroke_pt: 0.5,
                minimum_visible_length_pt: None,
            };
            let (arc, _) = materialize_arc(primitive, 1.0)
                .expect("a trimmed variable-radius arc should materialize");
            let first = arc.points_pt.first().expect("arc needs a start point");
            let last = arc.points_pt.last().expect("arc needs an end point");

            assert!(first.y > 0.0);
            assert!(first.x.hypot(first.y) > 10.0);
            assert!(last.x > 0.0);
            assert!(last.x.hypot(last.y) < 14.0);
        }

        #[test]
        fn variable_radius_arc_uses_its_true_length_for_clearance() {
            // A tapered span is longer than either endpoint radius suggests, so the
            // clearance test has to integrate the real polar length. This arc spans
            // only 0.5 radians but is over 11pt long, which is enough for both 3pt
            // gaps and the 5pt minimum.
            let roomy = PreparedArc {
                center: Point::new(0.0, 0.0),
                radius: 10.0,
                end_radius: Some(20.0),
                start_angle_rad: 0.0,
                span_rad: 0.5,
                endpoint_gap_pt: 3.0,
                half_stroke_pt: 0.5,
                minimum_visible_length_pt: Some(5.0),
            };
            let length = polar_arc_length(10.0, 20.0, 0.5, 0.0, 1.0);
            let (arc, _) =
                materialize_arc(roomy, 1.0).expect("the polar arc has enough length for both gaps");
            let first = arc.points_pt.first().expect("arc needs a start point");
            let last = arc.points_pt.last().expect("arc needs an end point");

            assert!(length > 11.0);
            assert!(first.x.hypot(first.y) > 10.0);
            assert!(last.x.hypot(last.y) < 20.0);

            // A quarter turn across a much smaller taper is genuinely too short for
            // the same clearance rules, so it must be dropped.
            let crowded = PreparedArc {
                center: Point::new(0.0, 0.0),
                radius: 1.0,
                end_radius: Some(2.0),
                start_angle_rad: 0.0,
                span_rad: FRAC_PI_2,
                endpoint_gap_pt: 3.0,
                half_stroke_pt: 0.5,
                minimum_visible_length_pt: Some(2.0),
            };
            assert!(materialize_arc(crowded, 1.0).is_none());
        }

        #[test]
        fn polar_arc_length_is_accurate_for_a_tapered_partial_span() {
            let length = polar_arc_length(
                0.938_041_412_334_400_6,
                0.010_813_764_562_194_73,
                2.004_308_037_238_873,
                0.140_670_844_679_762_12,
                0.953_191_962_765_845_6,
            );

            assert!((length - 1.064_260_298_103_157_7).abs() < 1e-10);
        }

        #[test]
        fn trimmed_arc_bounds_include_stroke_bleed() {
            let bounds_with_half_stroke = |half_stroke_pt: f64| {
                let request = FitRequest {
                    width_mode: WidthMode::Auto,
                    viewport_width_pt: None,
                    height_mode: HeightMode::Auto,
                    viewport_height_pt: None,
                    target_geometry_scale: None,
                    lines: Vec::new(),
                    arcs: vec![PreparedArc {
                        center: Point::new(0.0, 0.0),
                        radius: 10.0,
                        end_radius: None,
                        start_angle_rad: 0.0,
                        span_rad: FRAC_PI_2,
                        endpoint_gap_pt: 1.0,
                        half_stroke_pt,
                        minimum_visible_length_pt: None,
                    }],
                    curves: Vec::new(),
                    boxes: Vec::new(),
                    ticks: Vec::new(),
                };
                let plan = materialize(&request, 1.0).expect("arc should materialize");
                assert_eq!(
                    plan.arcs[0].as_ref().map(|arc| arc.points_pt.len()),
                    Some(4)
                );
                plan.bounds
            };

            // The stroke bleeds half its width past the path on every side.
            let bare = bounds_with_half_stroke(0.0);
            let stroked = bounds_with_half_stroke(0.5);
            assert!((stroked.width - bare.width - 1.0).abs() < GEOMETRY_EPSILON);
            assert!((stroked.height - bare.height - 1.0).abs() < GEOMETRY_EPSILON);
        }

        #[test]
        fn a_cubic_curve_is_materialized_and_bounded() {
            let json = br#"{
                "width_mode":"auto",
                "viewport_width_pt":null,
                "height_mode":"auto",
                "viewport_height_pt":null,
                "lines":[],
                "arcs":[],
                "curves":[{
                    "start":{"geometry":{"x":0,"y":0},"page_pt":{"x":0,"y":0}},
                    "control_1":{"x":0,"y":10},
                    "control_2":{"x":10,"y":10},
                    "end":{"geometry":{"x":10,"y":0},"page_pt":{"x":0,"y":0}},
                    "half_stroke_pt":1,
                    "min_scale":null
                }],
                "boxes":[],
                "ticks":[]
            }"#;
            let response: serde_json::Value =
                serde_json::from_slice(&fit(json).expect("curve fit should succeed"))
                    .expect("curve fit response should be JSON");
            let curve = &response["curves"][0];

            assert!((curve["start_pt"]["x"].as_f64().unwrap() - 1.0).abs() < GEOMETRY_EPSILON);
            assert!((curve["end_pt"]["x"].as_f64().unwrap() - 11.0).abs() < GEOMETRY_EPSILON);
            // The bulge, not the chord, sets the auto-sized viewport.
            assert!(response["viewport_height_pt"].as_f64().unwrap() > 7.0);
            assert!(response["viewport_width_pt"].as_f64().unwrap() >= 12.0);
        }
    }

    /// Nucleotide tick and label placement: preferred direction, obstacle
    /// avoidance, clearance at the geometry scale, and reservation sizing.
    mod tick_placement {
        use super::*;

        #[test]
        fn fixed_tick_and_label_overflow_constrain_the_scale() {
            let baseline = request(Some(120.0), None);
            let mut ticked = request(Some(120.0), None);
            ticked.ticks = vec![tick(
                Point::new(10.0, 0.0),
                Point::new(0.0, 0.0),
                Point::new(20.0, 0.0),
            )];

            let response = fit_request(&ticked).expect("tick overflow should be fitted");
            let without_tick = fit_request(&baseline).expect("baseline fit should succeed");

            // The fixed-size tick and label take room from the geometry.
            assert!(response.geometry_scale < without_tick.geometry_scale);
            assert_contained(&response);
            assert!(
                (response.occupied_bounds_pt.width - response.viewport_width_pt).abs()
                    <= FIT_TOLERANCE_PT
            );
        }

        #[test]
        fn tick_clearance_is_measured_at_the_geometry_scale() {
            // The obstacle sits one geometry unit from the tick's nucleotide, so
            // how far away it really is depends entirely on the geometry scale. At
            // At 60pt per unit the preferred side has room to spare. At 4pt per
            // unit the same obstacle is on top of the tick. A clearance test that
            // confuses geometry units with points would give the same answer twice.
            let roomy = blocked_tick_request(60.0);
            let cramped = blocked_tick_request(4.0);

            let roomy_response = fit_request(&roomy).expect("roomy fit should succeed");
            let cramped_response = fit_request(&cramped).expect("cramped fit should succeed");

            let roomy_side =
                roomy_response.ticks[0].top_left_pt.y - roomy_response.boxes[0].top_left_pt.y;
            let cramped_side =
                cramped_response.ticks[0].top_left_pt.y - cramped_response.boxes[0].top_left_pt.y;

            assert!(
                roomy_side > 0.0,
                "roomy tick should keep the preferred side"
            );
            assert!(
                cramped_side < 0.0,
                "cramped tick should cross to the free side, got {cramped_side}"
            );

            // Crossing over is only correct if it actually clears the obstacle.
            let blocker = Bounds::from_edges(
                cramped_response.boxes[1].top_left_pt.x,
                cramped_response.boxes[1].top_left_pt.y,
                cramped_response.boxes[1].top_left_pt.x + 26.0,
                cramped_response.boxes[1].top_left_pt.y + 26.0,
            );
            let label = label_bounds(&cramped.ticks[0], cramped_response.ticks[0]);
            assert!(
                !overlaps(label, blocker),
                "the label must clear the blocker"
            );
        }

        #[test]
        fn a_tick_leaves_a_legal_but_cramped_slot_for_an_open_one() {
            // The reported defect: a slot beside the preferred direction that clears
            // every obstacle by a hair, with open space on the far side. Fitting is
            // not the same as reading well, so the placement has to cross over.
            let mut request = request(None, None);
            request.target_geometry_scale = Some(20.0);
            request.lines.clear();
            request.boxes = vec![
                PreparedBox {
                    anchor: Point::new(0.0, 0.0),
                    width_pt: 6.0,
                    height_pt: 8.0,
                },
                // Two boxes straddling the preferred direction, leaving a corridor
                // the label fits through with about two points to spare.
                PreparedBox {
                    anchor: Point::new(-0.8, 1.2),
                    width_pt: 14.0,
                    height_pt: 40.0,
                },
                PreparedBox {
                    anchor: Point::new(0.8, 1.2),
                    width_pt: 14.0,
                    height_pt: 40.0,
                },
            ];
            request.ticks = vec![PreparedTick::new(RawTick {
                anchor: Point::new(0.0, 0.0),
                previous: Some(Point::new(-1.0, 0.0)),
                next: Some(Point::new(1.0, 0.0)),
                owner_box: Some(0),
                offset_pt: 6.0,
                tick_len_pt: 10.0,
                resolved_connector: None,
                gap_pt: 1.0,
                half_stroke_pt: 0.5,
                label_width_pt: 12.0,
                label_height_pt: 8.0,
            })];

            let response = fit_request(&request).expect("cramped tick should be placed");
            let clearance = label_clearance(&request, &response, 0);

            assert!(
                clearance >= request.ticks[0].offset_pt,
                "placement should be comfortable, got {clearance}pt of clearance"
            );
            // The corridor runs below the nucleotide, so the open side is above it.
            assert!(response.ticks[0].top_left_pt.y < response.boxes[0].top_left_pt.y);
        }

        #[test]
        fn terminal_labels_turn_into_open_space() {
            let mut request = request(None, None);
            request.target_geometry_scale = Some(20.0);
            request.lines.clear();
            request.boxes = vec![
                PreparedBox {
                    anchor: Point::new(0.0, 0.0),
                    width_pt: 6.0,
                    height_pt: 8.0,
                },
                // The terminal naturally points upward, but the structure occupies
                // that side. The label must choose a clear direction instead.
                PreparedBox {
                    anchor: Point::new(0.0, -1.0),
                    width_pt: 32.0,
                    height_pt: 16.0,
                },
            ];
            request.ticks = vec![PreparedTick::new(RawTick {
                anchor: Point::new(0.0, 0.0),
                previous: Some(Point::new(0.0, 1.0)),
                next: None,
                owner_box: Some(0),
                offset_pt: 6.0,
                tick_len_pt: 0.0,
                resolved_connector: None,
                gap_pt: 1.0,
                half_stroke_pt: 0.0,
                label_width_pt: 12.0,
                label_height_pt: 8.0,
            })];

            let response = fit_request(&request).expect("terminal label should be placed");
            let label = label_bounds(&request.ticks[0], response.ticks[0]);
            let blocker = Bounds::from_edges(-16.0, -28.0, 16.0, -12.0);

            assert!(!overlaps(label, blocker));
            assert!(label.max_y > blocker.max_y);
        }

        #[test]
        fn an_unobstructed_tick_keeps_its_direction_and_its_requested_length() {
            let mut request = request(None, None);
            request.target_geometry_scale = Some(40.0);
            request.lines.clear();
            request.boxes = vec![PreparedBox {
                anchor: Point::new(0.0, 0.0),
                width_pt: 6.0,
                height_pt: 8.0,
            }];
            request.ticks = vec![PreparedTick::new(RawTick {
                anchor: Point::new(0.0, 0.0),
                previous: Some(Point::new(-1.0, 0.0)),
                next: Some(Point::new(0.0, -1.0)),
                owner_box: Some(0),
                offset_pt: 6.0,
                tick_len_pt: 10.0,
                resolved_connector: None,
                gap_pt: 1.0,
                half_stroke_pt: 0.5,
                label_width_pt: 12.0,
                label_height_pt: 8.0,
            })];

            let response = fit_request(&request).expect("free tick should be placed");
            let line = response.ticks[0]
                .line
                .expect("a tick with length draws one");
            let length = (line.end_pt.x - line.start_pt.x).hypot(line.end_pt.y - line.start_pt.y);

            // With nothing nearby the deficit is zero everywhere, so the winner is
            // the zero-cost shape: the preferred direction at the requested length.
            assert!((length - 10.0).abs() < GEOMETRY_EPSILON);
            let (preferred, _) = ticks::preferred_direction(
                Point::new(0.0, 0.0),
                Some(Point::new(-1.0, 0.0)),
                Some(Point::new(0.0, -1.0)),
            );
            let start = line.start_pt.translated(Point::new(
                -response.translation_pt.x,
                -response.translation_pt.y,
            ));
            assert!((start.x - preferred.x * 6.0).abs() < 1e-9);
            assert!((start.y - preferred.y * 6.0).abs() < 1e-9);
        }

        #[test]
        fn ticks_placed_in_sequence_do_not_stack_on_each_other() {
            let mut request = request(None, None);
            request.lines.clear();
            // Keep the only box far from both ticks. A box at the anchors would
            // separate the labels by itself, so the test would pass even if a
            // placed tick reserved nothing.
            request.boxes = vec![PreparedBox {
                anchor: Point::new(100.0, 0.0),
                width_pt: 6.0,
                height_pt: 8.0,
            }];
            // These nucleotides share a preferred direction, so the second tick must
            // move aside.
            request.ticks = vec![
                tick(
                    Point::new(0.0, 0.0),
                    Point::new(-10.0, 0.0),
                    Point::new(10.0, 0.0),
                ),
                tick(
                    Point::new(1.0, 0.0),
                    Point::new(-9.0, 0.0),
                    Point::new(11.0, 0.0),
                ),
            ];

            let response = fit_request(&request).expect("two ticks should be placed");

            assert!(!overlaps(
                label_bounds(&request.ticks[0], response.ticks[0]),
                label_bounds(&request.ticks[1], response.ticks[1]),
            ));
        }

        #[test]
        fn a_fully_enclosed_tick_still_gets_its_least_bad_placement() {
            let mut request = request(None, None);
            request.lines.clear();
            request.boxes = vec![PreparedBox {
                anchor: Point::new(0.0, 0.0),
                width_pt: 200.0,
                height_pt: 200.0,
            }];
            request.ticks = vec![tick(
                Point::new(0.0, 0.0),
                Point::new(-1.0, 0.0),
                Point::new(1.0, 0.0),
            )];

            let response = fit_request(&request).expect("an enclosed tick must not fail the fit");

            assert_eq!(response.ticks.len(), 1);
            assert!(response.ticks[0].top_left_pt.x.is_finite());
            assert!(response.ticks[0].top_left_pt.y.is_finite());
        }

        #[test]
        fn tick_reservations_are_placement_independent_and_never_undersized() {
            let mut request = request(None, None);
            request.ticks = vec![tick(
                Point::new(5.0, 0.0),
                Point::new(0.0, 0.0),
                Point::new(10.0, 0.0),
            )];

            let plan = materialize(&request, 1.0).expect("plan should materialize");
            let reserved = plan.bounds;
            let response = fit_request(&request).expect("natural fit should succeed");

            // The reservation covers every direction, so the final bounds cannot
            // exceed it.
            assert!(reserved.width >= response.occupied_bounds_pt.width - FIT_TOLERANCE_PT);
            assert!(reserved.height >= response.occupied_bounds_pt.height - FIT_TOLERANCE_PT);
            let rotated = tick(
                Point::new(5.0, 0.0),
                Point::new(5.0, -10.0),
                Point::new(5.0, 10.0),
            );
            let rotated_reservation = ticks::reservation(&rotated, 1.0);
            let original_reservation = ticks::reservation(&request.ticks[0], 1.0);
            assert!((rotated_reservation.width - original_reservation.width).abs() < 1e-9);
            assert!((rotated_reservation.height - original_reservation.height).abs() < 1e-9);
        }

        #[test]
        fn a_tick_ignores_the_box_of_its_own_nucleotide() {
            // The owner box is tall enough to cover both normals of the backbone,
            // so counting it as an obstacle would push the tick sideways. Ignoring
            // it leaves the preferred direction free. The other tick tests cover
            // avoidance of non-owner boxes.
            let mut request = request(None, None);
            request.lines.clear();
            request.boxes = vec![PreparedBox {
                anchor: Point::new(0.0, 0.0),
                width_pt: 8.0,
                height_pt: 60.0,
            }];
            request.ticks = vec![PreparedTick::new(RawTick {
                anchor: Point::new(0.0, 0.0),
                previous: Some(Point::new(-1.0, 0.0)),
                next: Some(Point::new(1.0, 0.0)),
                owner_box: Some(0),
                offset_pt: 6.0,
                tick_len_pt: 10.0,
                resolved_connector: None,
                gap_pt: 1.0,
                half_stroke_pt: 0.5,
                label_width_pt: 12.0,
                label_height_pt: 8.0,
            })];
            request.target_geometry_scale = Some(20.0);
            let response = fit_request(&request).expect("tick with owner must be placed");
            let line = response.ticks[0]
                .line
                .expect("a tick with length draws one");
            let (preferred, _) = ticks::preferred_direction(
                Point::new(0.0, 0.0),
                Some(Point::new(-1.0, 0.0)),
                Some(Point::new(1.0, 0.0)),
            );
            let start = line.start_pt.translated(Point::new(
                -response.translation_pt.x,
                -response.translation_pt.y,
            ));
            assert!(
                (start.x - preferred.x * 6.0).abs() < 1e-9
                    && (start.y - preferred.y * 6.0).abs() < 1e-9,
                "the owner box must not deflect the tick, which started at {start:?}"
            );
        }
    }

    /// Rejection of malformed `fit` requests at the wire boundary.
    mod wire_validation {
        use super::*;

        #[test]
        fn invalid_wire_values_are_rejected() {
            let json = br#"{
                "width_mode":"resolved",
                "viewport_width_pt":-1,
                "height_mode":"auto",
                "viewport_height_pt":null,
                "lines":[],
                "arcs":[],
                "boxes":[{"anchor":{"x":0,"y":0},"width_pt":1,"height_pt":1}]
            }"#;
            let error = fit(json).expect_err("negative viewport width must fail");
            assert!(matches!(error, FitError::InvalidRequest(_)));
        }

        #[test]
        fn non_positive_minimum_visible_length_is_rejected() {
            let json = br#"{
                "width_mode":"auto",
                "viewport_width_pt":null,
                "height_mode":"auto",
                "viewport_height_pt":null,
                "lines":[{
                    "start":{"geometry":{"x":0,"y":0},"page_pt":{"x":0,"y":0}},
                    "end":{"geometry":{"x":1,"y":0},"page_pt":{"x":0,"y":0}},
                    "half_stroke_pt":0.5,
                    "min_scale":null,
                    "minimum_visible_length_pt":0
                }],
                "arcs":[],
                "boxes":[]
            }"#;

            let error = fit(json).expect_err("a zero visibility floor must fail");

            assert!(matches!(error, FitError::InvalidRequest(_)));
            assert!(
                error
                    .to_string()
                    .contains("minimum_visible_length_pt must be positive")
            );
        }

        #[test]
        fn resolved_viewport_requires_its_dimension() {
            let json = br#"{
                "width_mode":"resolved",
                "viewport_width_pt":null,
                "height_mode":"auto",
                "viewport_height_pt":null,
                "lines":[],
                "arcs":[],
                "boxes":[{"anchor":{"x":0,"y":0},"width_pt":1,"height_pt":1}]
            }"#;

            let error = fit(json).expect_err("missing resolved width must fail");

            assert!(matches!(error, FitError::InvalidRequest(_)));
            assert!(error.to_string().contains("viewport_width_pt is required"));
        }
    }
}
