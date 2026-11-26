//! The progress bars of a download, an import or a write, with how long each step takes
//! (`ProgressBar.svelte` of the earlier window).

use std::time::{Duration, Instant};

use eframe::egui::Ui;

use crate::make::format;
use crate::make::stick_job::Progress;
use crate::widgets;

/// When a step began and, once its last byte is done, when it ended. Writing and checking go on
/// together, so each has its own clock.
#[derive(Clone, Copy)]
struct Clock {
    start: Instant,
    end: Option<Instant>,
}

#[derive(Default)]
pub struct Bar {
    progress: Option<Progress>,
    main: Option<Clock>,
    check: Option<Clock>,
}

fn label(phase: &str) -> &'static str {
    match phase {
        "downloading" => "Downloading Keeper OS",
        "importing" => "Verifying and expanding the download",
        "building" => "Copying the image",
        "checking" => "Checking",
        _ => "Writing",
    }
}

/// `4:12 · 9.5 MB/s · about 6:30 left`, or `took 14:32` once the step is done.
fn timing(clock: Option<Clock>, done: u64, total: u64, now: Instant) -> String {
    let Some(clock) = clock else {
        return String::new();
    };
    let seconds = clock
        .end
        .unwrap_or(now)
        .saturating_duration_since(clock.start)
        .as_secs();
    if clock.end.is_some() {
        return format!("took {}", format::duration(seconds));
    }
    let mut parts = vec![format::duration(seconds)];
    // Nothing is told about the speed until there are a few seconds to measure.
    if seconds >= 3 && done > 0 {
        parts.push(format!("{}/s", format::bytes(done / seconds)));
    }
    if seconds >= 5 && done > 0 && done < total {
        let left = u128::from(total - done) * u128::from(seconds) / u128::from(done);
        let left = u64::try_from(left).unwrap_or(u64::MAX);
        parts.push(format!("about {} left", format::duration(left)));
    }
    parts.join(" \u{b7} ")
}

impl Bar {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Takes the latest report in, `now` being when it came.
    pub fn update(&mut self, progress: Progress, now: Instant) {
        let seen = self.progress.map(|last| last.phase);
        let fresh = Clock {
            start: now,
            end: None,
        };
        // Checking after writing is the same operation; any other new kind of step starts over.
        let goes_on = seen == Some("writing") && progress.phase == "checking";
        // Each stick's image is copied from the start, all under the same phase.
        let next_copy = progress.phase == "building"
            && self
                .progress
                .is_some_and(|last| last.done >= last.total || progress.done < last.done);
        if (seen != Some(progress.phase) && !goes_on) || next_copy {
            self.main = Some(fresh);
            self.check = None;
        }
        self.progress = Some(progress);
        let end = |clock: &mut Option<Clock>, done: u64| {
            if let Some(clock) = clock
                && clock.end.is_none()
                && progress.total > 0
                && done >= progress.total
            {
                clock.end = Some(now);
            }
        };
        end(&mut self.main, progress.done);
        if progress.checked > 0 && self.check.is_none() {
            self.check = Some(fresh);
        }
        end(&mut self.check, progress.checked);
    }

    pub fn show(&self, ui: &mut Ui) {
        let Some(progress) = self.progress else {
            return;
        };
        let now = Instant::now();
        let total = format::bytes(progress.total);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let mut line = |name: &str, done: u64, clock: Option<Clock>| {
                widgets::split_line(
                    ui,
                    &format!("{name}: {} of {total}", format::bytes(done)),
                    &timing(clock, done, progress.total, now),
                );
                widgets::progress_bar(ui, format::fraction(done, progress.total));
            };
            line(label(progress.phase), progress.done, self.main);
            if matches!(progress.phase, "writing" | "checking") {
                line("Checked", progress.checked, self.check);
            }
        });
        // The clocks go on between reports.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(phase: &'static str, done: u64, checked: u64) -> Progress {
        Progress {
            phase,
            done,
            total: 100,
            checked,
        }
    }

    #[test]
    fn each_image_copy_gets_its_own_clock() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut bar = Bar::default();
        bar.update(report("building", 50, 0), at(0));
        bar.update(report("building", 100, 0), at(4));
        bar.update(report("building", 10, 0), at(5));
        assert_eq!(timing(bar.main, 10, 100, at(7)), "0:02");
    }

    #[test]
    fn writing_and_checking_keep_their_clocks_and_a_new_step_starts_over() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut bar = Bar::default();
        bar.update(report("building", 50, 0), at(0));
        bar.update(report("writing", 10, 0), at(2));
        bar.update(report("writing", 20, 5), at(4));
        bar.update(report("checking", 100, 50), at(10));
        assert_eq!(timing(bar.main, 100, 100, at(60)), "took 0:08");
        assert_eq!(
            timing(bar.check, 50, 100, at(14)),
            "0:10 \u{b7} 5 B/s \u{b7} about 0:10 left"
        );
        assert_eq!(timing(bar.check, 50, 100, at(6)), "0:02");
        bar.update(report("building", 1, 0), at(20));
        assert!(bar.check.is_none());
        assert_eq!(timing(bar.main, 1, 100, at(20)), "0:00");
    }
}
