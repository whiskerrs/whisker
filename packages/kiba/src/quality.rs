/// A rendering feature level. The game picks one per device while running, and it is never a
/// different set of assets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Mid-range phones: 30 fps, up to 1280 pixels on the long side, with antialiasing, bloom, and two
    /// shadow cascades.
    T0,
    /// High-end phones: 60 fps, up to 1920 pixels, adding ambient occlusion, light shafts, and three
    /// cascades of soft shadows.
    T1,
    /// Desktops: 60 fps at any size, with stronger ambient occlusion and four cascades of soft shadows.
    #[default]
    T2,
}

impl Tier {
    fn target_ms(self) -> f32 {
        match self {
            Self::T0 => 1000.0 / 30.0,
            Self::T1 | Self::T2 => 1000.0 / 60.0,
        }
    }

    fn longest_side(self) -> f32 {
        match self {
            Self::T0 => 1280.0,
            Self::T1 => 1920.0,
            Self::T2 => f32::MAX,
        }
    }

    // Lower tiers trade more resolution, because there is nothing below them to fall back to.
    fn most_steps_down(self) -> u32 {
        match self {
            Self::T0 => 5,
            Self::T1 | Self::T2 => 3,
        }
    }

    fn higher(self) -> Self {
        match self {
            Self::T0 => Self::T1,
            Self::T1 | Self::T2 => Self::T2,
        }
    }

    fn lower(self) -> Option<Self> {
        match self {
            Self::T2 => Some(Self::T1),
            Self::T1 => Some(Self::T0),
            Self::T0 => None,
        }
    }
}

/// What the renderer does this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Quality {
    /// Internal resolution as a fraction of the output.
    pub(crate) render_scale: f32,
    pub(crate) bloom: f32,
    pub(crate) ambient_occlusion: f32,
    pub(crate) shadow_cascades: usize,
    /// Texels per side of each cascade's shadow map.
    pub(crate) shadow_map_size: u32,
    pub(crate) soft_shadows: bool,
    pub(crate) antialias: bool,
    /// Strength of the sun's rays through gaps in what stands in front of the sky.
    pub(crate) light_shafts: f32,
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            render_scale: 1.0,
            bloom: 0.0,
            ambient_occlusion: 0.0,
            shadow_cascades: 2,
            shadow_map_size: 1024,
            soft_shadows: false,
            antialias: false,
            light_shafts: 0.0,
        }
    }
}

const WINDOW_SECONDS: f32 = 1.0;
// The tab was hidden or a load hitched, which says nothing about how fast the GPU is.
const LONGEST_COUNTED_FRAME_SECONDS: f32 = 0.25;
const SCALE_STEP: f32 = 0.1;
// A frame this far past the budget waited for another refresh.
const OVER_BUDGET: f32 = 1.1;
// Under vsync a late frame takes a whole extra refresh, so a few of them already show as stutter
// that a median would hide.
const MOST_LATE_SHARE: f32 = 0.1;
// A display's refresh caps how fast frames arrive, so frames meeting the budget is the only sign of
// room, and one late frame a window is a load hitch rather than a lack of it.
const MOST_LATE_FRAMES_WHEN_CALM: usize = 1;
// Giving up resolution is quick and taking it back is slow, so the image does not pulse.
const WINDOWS_BEFORE_RAISING: u32 = 3;
const MOST_WINDOWS_BEFORE_RAISING: u32 = 64;
// A raise that fails within this many windows was too much, even if its first window passed.
const WINDOWS_TO_TRUST_A_RAISE: u32 = 5;
// A burst of heavy action costs resolution for a moment, but only sustained load costs a tier.
const WINDOWS_BEFORE_LOWERING_TIER: u32 = 3;
// The output must shrink this much below where a tier was lost before that tier is tried again.
const OUTPUT_SHRINK_TO_RETRY: f32 = 0.75;

/// Chooses a tier and an internal resolution from how long frames really take.
///
/// It starts at the top and steps down, because a benchmark before the game starts would not show
/// the game's own load.
#[derive(Debug)]
pub(crate) struct Governor {
    tier: Tier,
    locked: bool,
    // Counted in steps, because repeatedly adding tenths drifts past the limits.
    steps_down: u32,
    elapsed: f32,
    frame_ms: Vec<f32>,
    calm_windows: u32,
    windows_before_raising: u32,
    windows_since_raise: Option<u32>,
    strained_windows: u32,
    output_pixels: f32,
    // The output size at each tier given up, the latest last.
    tiers_lost_at: Vec<f32>,
}

impl Default for Governor {
    fn default() -> Self {
        Self {
            tier: Tier::T2,
            locked: false,
            steps_down: 0,
            elapsed: 0.0,
            frame_ms: Vec::new(),
            calm_windows: 0,
            windows_before_raising: WINDOWS_BEFORE_RAISING,
            windows_since_raise: None,
            strained_windows: 0,
            output_pixels: 0.0,
            tiers_lost_at: Vec::new(),
        }
    }
}

impl Governor {
    pub(crate) fn lock(&mut self, tier: Tier) {
        self.tier = tier;
        self.locked = true;
        self.steps_down = 0;
        self.tiers_lost_at.clear();
    }

    /// Takes back each tier lost at an output much larger than `width` by `height`.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.output_pixels = width as f32 * height as f32;
        while self
            .tiers_lost_at
            .last()
            .is_some_and(|&lost_at| self.output_pixels < lost_at * OUTPUT_SHRINK_TO_RETRY)
        {
            self.tiers_lost_at.pop();
            self.tier = self.tier.higher();
            self.steps_down = 0;
            self.strained_windows = 0;
        }
    }

    pub(crate) fn tier(&self) -> Tier {
        self.tier
    }

    pub(crate) fn update(&mut self, delta_seconds: f32) {
        if delta_seconds <= 0.0 || delta_seconds > LONGEST_COUNTED_FRAME_SECONDS {
            return;
        }
        self.elapsed += delta_seconds;
        self.frame_ms.push(delta_seconds * 1000.0);
        if self.elapsed < WINDOW_SECONDS {
            return;
        }
        let target = self.tier.target_ms();
        let late = self
            .frame_ms
            .iter()
            .filter(|&&ms| ms > target * OVER_BUDGET)
            .count();
        let late_share = late as f32 / self.frame_ms.len() as f32;
        self.elapsed = 0.0;
        self.frame_ms.clear();
        let since_raise = self.windows_since_raise.take().map(|windows| windows + 1);
        if late_share > MOST_LATE_SHARE {
            self.calm_windows = 0;
            if since_raise.is_some() {
                self.windows_before_raising =
                    (self.windows_before_raising * 2).min(MOST_WINDOWS_BEFORE_RAISING);
            }
            if self.steps_down < self.tier.most_steps_down() {
                self.steps_down += 1;
                self.strained_windows = 0;
            } else if let Some(lower) = self.tier.lower().filter(|_| !self.locked) {
                self.strained_windows += 1;
                if self.strained_windows >= WINDOWS_BEFORE_LOWERING_TIER {
                    self.tiers_lost_at.push(self.output_pixels);
                    self.tier = lower;
                    self.steps_down = 0;
                    self.strained_windows = 0;
                }
            }
            return;
        }
        self.strained_windows = 0;
        self.windows_since_raise =
            since_raise.filter(|&windows| windows < WINDOWS_TO_TRUST_A_RAISE);
        if late <= MOST_LATE_FRAMES_WHEN_CALM {
            self.calm_windows += 1;
            if self.calm_windows >= self.windows_before_raising && self.steps_down > 0 {
                self.calm_windows = 0;
                self.steps_down -= 1;
                self.windows_since_raise = Some(0);
            }
        } else {
            self.calm_windows = 0;
        }
    }

    pub(crate) fn quality(&self, width: u32, height: u32) -> Quality {
        let side = width.max(height).max(1) as f32;
        let cap = (self.tier.longest_side() / side).min(1.0);
        let (bloom, ambient_occlusion, light_shafts) = match self.tier {
            Tier::T0 => (0.04, 0.0, 0.0),
            Tier::T1 => (0.04, 0.6, 0.3),
            Tier::T2 => (0.05, 0.9, 0.3),
        };
        let (shadow_cascades, shadow_map_size, soft_shadows) = match self.tier {
            Tier::T0 => (2, 1024, false),
            Tier::T1 => (3, 2048, true),
            Tier::T2 => (4, 2048, true),
        };
        Quality {
            render_scale: self.scale().min(cap),
            bloom,
            ambient_occlusion,
            shadow_cascades,
            shadow_map_size,
            soft_shadows,
            antialias: true,
            light_shafts,
        }
    }

    fn scale(&self) -> f32 {
        1.0 - self.steps_down as f32 * SCALE_STEP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(governor: &mut Governor, frame_ms: f32, seconds: f32) {
        for _ in 0..(seconds * 1000.0 / frame_ms) as usize {
            governor.update(frame_ms / 1000.0);
        }
    }

    // Feeds whole windows, so a window never mixes two loads.
    fn windows(governor: &mut Governor, frame_ms: f32, count: usize) {
        for _ in 0..count {
            governor.update(frame_ms / 1000.0);
            while governor.elapsed > 0.0 {
                governor.update(frame_ms / 1000.0);
            }
        }
    }

    #[test]
    fn a_fast_device_keeps_the_top_tier_at_full_resolution() {
        let mut governor = Governor::default();
        run(&mut governor, 16.6, 30.0);
        assert_eq!(governor.tier(), Tier::T2);
        assert_eq!(governor.scale(), 1.0);
    }

    #[test]
    fn a_slow_device_gives_up_resolution_and_then_tiers_until_it_fits() {
        let mut governor = Governor::default();
        run(&mut governor, 28.0, 3.5);
        assert_eq!(governor.tier(), Tier::T2);
        assert!(governor.scale() < 1.0);

        run(&mut governor, 28.0, 30.0);
        assert_eq!(
            governor.tier(),
            Tier::T0,
            "28 ms meets only the 33 ms budget"
        );
    }

    #[test]
    fn t0_holds_at_its_lowest_resolution_on_a_device_slower_than_every_tier() {
        let mut governor = Governor::default();
        run(&mut governor, 100.0, 120.0);
        assert_eq!(governor.tier(), Tier::T0);
        assert_eq!(governor.scale(), 0.5);
    }

    #[test]
    fn resolution_returns_slowly_once_there_is_room() {
        let mut governor = Governor::default();
        windows(&mut governor, 28.0, 2);
        let lowered = governor.scale();
        windows(&mut governor, 16.7, 2);
        assert_eq!(
            governor.scale(),
            lowered,
            "a display's refresh is room enough, but only slowly"
        );
        windows(&mut governor, 16.7, 1);
        assert!(governor.scale() > lowered);
    }

    #[test]
    fn a_hitch_in_each_window_changes_nothing() {
        let mut governor = Governor::default();
        for _ in 0..30 {
            governor.update(0.2);
            run(&mut governor, 16.7, 0.8);
        }
        assert_eq!(governor.tier(), Tier::T2);
        assert_eq!(governor.scale(), 1.0);
    }

    #[test]
    fn resolution_returns_through_a_hitch_in_each_window() {
        let mut governor = Governor::default();
        windows(&mut governor, 28.0, 2);
        let lowered = governor.scale();
        for _ in 0..4 {
            governor.update(0.2);
            windows(&mut governor, 16.7, 1);
        }
        assert!(governor.scale() > lowered);
    }

    #[test]
    fn a_raise_that_fails_waits_longer_before_the_next() {
        let mut governor = Governor::default();
        windows(&mut governor, 20.0, 1);
        assert_eq!(governor.scale(), 0.9);
        windows(&mut governor, 16.7, 3);
        assert_eq!(governor.scale(), 1.0);
        windows(&mut governor, 20.0, 1);
        assert_eq!(governor.scale(), 0.9);
        windows(&mut governor, 16.7, 5);
        assert_eq!(governor.scale(), 0.9, "the second try waits six windows");
    }

    #[test]
    fn a_fifth_of_frames_missing_the_refresh_gives_up_resolution() {
        let mut governor = Governor::default();
        for _ in 0..12 {
            run(&mut governor, 16.7, 0.067);
            governor.update(0.0333);
        }
        assert!(governor.scale() < 1.0);
    }

    #[test]
    fn a_raise_that_fails_a_few_windows_later_still_waits_longer() {
        let mut governor = Governor::default();
        windows(&mut governor, 20.0, 1);
        windows(&mut governor, 16.7, 3);
        assert_eq!(governor.scale(), 1.0);
        windows(&mut governor, 16.7, 2);
        windows(&mut governor, 20.0, 1);
        assert_eq!(governor.scale(), 0.9);
        windows(&mut governor, 16.7, 5);
        assert_eq!(governor.scale(), 0.9, "the second try waits six windows");
    }

    #[test]
    fn a_short_spike_at_the_lowest_resolution_keeps_the_tier() {
        let mut governor = Governor::default();
        windows(&mut governor, 33.3, 3);
        assert_eq!(governor.scale(), 0.7);
        windows(&mut governor, 33.3, 2);
        windows(&mut governor, 16.7, 1);
        assert_eq!(governor.tier(), Tier::T2);
    }

    #[test]
    fn a_tier_lost_to_a_larger_output_returns_when_it_shrinks() {
        let mut governor = Governor::default();
        governor.resize(3456, 2234);
        run(&mut governor, 33.3, 10.0);
        assert!(governor.tier() < Tier::T2);
        governor.resize(1728, 1117);
        assert_eq!(governor.tier(), Tier::T2);
    }

    #[test]
    fn a_locked_tier_only_changes_resolution() {
        let mut governor = Governor::default();
        governor.lock(Tier::T1);
        run(&mut governor, 100.0, 60.0);
        assert_eq!(governor.tier(), Tier::T1);
        assert!(governor.scale() < 1.0);
    }

    #[test]
    fn hidden_tabs_do_not_count_against_the_device() {
        let mut governor = Governor::default();
        for _ in 0..50 {
            governor.update(5.0);
        }
        assert_eq!(governor.tier(), Tier::T2);
        assert_eq!(governor.scale(), 1.0);
    }

    #[test]
    fn a_tier_caps_internal_resolution_by_the_long_side() {
        let mut governor = Governor::default();
        governor.lock(Tier::T0);
        assert_eq!(governor.quality(2560, 1440).render_scale, 0.5);
        assert_eq!(governor.quality(640, 480).render_scale, 1.0);
        governor.lock(Tier::T2);
        assert_eq!(governor.quality(2560, 1440).render_scale, 1.0);
    }

    #[test]
    fn higher_tiers_add_features() {
        let mut governor = Governor::default();
        governor.lock(Tier::T0);
        assert_eq!(governor.quality(100, 100).ambient_occlusion, 0.0);
        assert!(governor.quality(100, 100).bloom > 0.0);
        governor.lock(Tier::T1);
        assert!(governor.quality(100, 100).ambient_occlusion > 0.0);
    }
}
