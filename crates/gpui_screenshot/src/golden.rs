use std::{
    env, fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};

use crate::{
    Difference, Screenshot, Tolerance, compare, diff_image,
    output::{self, PRINT_VAR, UPDATE_VAR},
};

/// Environment variables that change what a screenshot looks like: the first three how GPUI
/// rasterizes text, `SEED` the order the test scheduler runs tasks in.
const RENDERING_VARS: [&str; 4] = [
    "ZED_FONTS_GAMMA",
    "ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST",
    "ZED_FONTS_SUBPIXEL_ENHANCED_CONTRAST",
    "SEED",
];

/// The goldens in `tests/screenshots` of the crate the macro is used in.
#[macro_export]
macro_rules! goldens {
    () => {
        $crate::Goldens::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/screenshots"))
    };
}

/// A folder of golden PNGs to compare screenshots with.
///
/// A golden named `button` is `button.png` in the folder. When a screenshot differs from it, or
/// it doesn't exist, the actual image and a diff go to `failures/button.actual.png` and
/// `failures/button.diff.png`, a subfolder that ignores itself in git. With `UPDATE_GOLDENS=1`
/// the screenshot becomes the golden instead, unless it already matches.
#[derive(Clone, Debug)]
pub struct Goldens {
    dir: PathBuf,
    tolerance: Tolerance,
}

impl Goldens {
    /// Goldens in `dir`, matched exactly. [`goldens!`](crate::goldens) gives the conventional
    /// `tests/screenshots` folder of the calling crate.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            tolerance: Tolerance::EXACT,
        }
    }

    /// Lets screenshots differ from their goldens by up to `tolerance`.
    pub fn tolerance(self, tolerance: Tolerance) -> Self {
        Self { tolerance, ..self }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the golden called `name` lives.
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.png"))
    }

    /// Compares `screenshot` with the golden called `name`. Names may use letters, digits, `-`,
    /// `_`, `.` and `@`, with `/` between subfolders.
    ///
    /// Fails with a [`Mismatch`] when the golden is missing or differs, or with the I/O error
    /// that stopped the comparison.
    pub fn check(&self, name: &str, screenshot: &Screenshot) -> Result<()> {
        self.check_with(name, screenshot, output::flag(UPDATE_VAR))
    }

    fn check_with(&self, name: &str, screenshot: &Screenshot, update: bool) -> Result<()> {
        validate(name)?;
        let golden = self.path(name);
        let failures = self.dir.join("failures");
        let actual_path = failures.join(format!("{name}.actual.png"));
        let diff_path = failures.join(format!("{name}.diff.png"));
        let expected = match fs::read(&golden) {
            Ok(bytes) => Some(
                output::decode_png(&bytes)
                    .with_context(|| format!("reading golden {}", golden.display()))?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("reading golden {}", golden.display()));
            }
        };
        let difference = expected
            .as_ref()
            .map(|expected| compare(&expected.image, &screenshot.image, self.tolerance));
        let matches = difference.is_some_and(|difference| difference.within(self.tolerance));

        if matches || update {
            if !matches {
                output::write(
                    &golden,
                    &output::encode_png(&screenshot.image, &screenshot.source)?,
                )?;
                eprintln!("updated golden {}", golden.display());
            }
            output::remove(&actual_path)?;
            output::remove(&diff_path)?;
            return Ok(());
        }

        // Keeps the failures out of git without an entry in the repository's .gitignore.
        fs::create_dir_all(&failures).ok();
        fs::write(failures.join(".gitignore"), "*\n").ok();
        output::write(
            &actual_path,
            &output::encode_png(&screenshot.image, &screenshot.source)?,
        )?;
        let diff = match &expected {
            Some(expected) => diff_image(&expected.image, &screenshot.image, self.tolerance),
            None => None,
        };
        let diff_path = match diff {
            Some(diff) => {
                output::write(&diff_path, &output::encode_png(&diff, &screenshot.source)?)?;
                Some(diff_path)
            }
            None => {
                output::remove(&diff_path)?;
                None
            }
        };
        Err(Mismatch {
            name: name.to_string(),
            golden,
            difference,
            tolerance: self.tolerance,
            golden_source: expected.and_then(|expected| expected.source),
            actual_source: screenshot.source.clone(),
            actual: actual_path,
            diff: diff_path,
        }
        .into())
    }

    /// [`Self::check`], panicking with the report on failure.
    #[track_caller]
    pub fn assert(&self, name: &str, screenshot: &Screenshot) {
        if let Err(error) = self.check(name, screenshot) {
            panic!("{error:#}");
        }
    }
}

/// A screenshot that has no golden, or differs from it by more than the tolerance.
#[derive(Clone, Debug)]
pub struct Mismatch {
    pub name: String,
    pub golden: PathBuf,
    /// How the screenshot differs, `None` when there is no golden.
    pub difference: Option<Difference>,
    pub tolerance: Tolerance,
    /// The adapter recorded in the golden.
    pub golden_source: Option<String>,
    /// The adapter that drew the screenshot.
    pub actual_source: String,
    /// Where the screenshot was written.
    pub actual: PathBuf,
    /// Where the diff image was written, when the sizes match.
    pub diff: Option<PathBuf>,
}

impl std::error::Error for Mismatch {}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.name;
        let golden = self.golden.display();
        let Some(difference) = self.difference else {
            writeln!(f, "screenshot `{name}` has no golden at {golden}")?;
            writeln!(f, "  actual: {}", self.actual.display())?;
            writeln!(f, "  drawn by: {}", self.actual_source)?;
            write!(
                f,
                "Check the image, then run with {UPDATE_VAR}=1 to make it the golden."
            )?;
            return self.notes(f);
        };
        if difference.same_size() {
            let total = difference.actual_size.0 as usize * difference.actual_size.1 as usize;
            writeln!(f, "screenshot `{name}` differs from its golden {golden}")?;
            writeln!(
                f,
                "  {} of {} pixels changed, the largest channel difference is {}",
                difference.changed, total, difference.max_channel
            )?;
            if self.tolerance != Tolerance::EXACT {
                writeln!(
                    f,
                    "  {} pixels differ by more than {} and the tolerance allows {}",
                    difference.over, self.tolerance.channel, self.tolerance.pixels
                )?;
            }
        } else {
            let (width, height) = difference.actual_size;
            let (golden_width, golden_height) = difference.expected_size;
            writeln!(
                f,
                "screenshot `{name}` is {width}x{height} pixels and its golden {golden} is \
                 {golden_width}x{golden_height}"
            )?;
        }
        let golden_source = self.golden_source.as_deref().unwrap_or("not recorded");
        writeln!(f, "  golden drawn by: {golden_source}")?;
        writeln!(f, "  this run:        {}", self.actual_source)?;
        writeln!(f, "  actual: {}", self.actual.display())?;
        if let Some(diff) = &self.diff {
            writeln!(
                f,
                "  diff:   {} (red: over the tolerance, amber: within it)",
                diff.display()
            )?;
        }
        write!(
            f,
            "Check the images, then run with {UPDATE_VAR}=1 to make the actual image the golden."
        )?;
        self.notes(f)
    }
}

impl Mismatch {
    fn notes(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !output::flag(PRINT_VAR) {
            write!(
                f,
                "\nOn a machine whose files don't come back, {PRINT_VAR}=1 prints the images."
            )?;
        }
        for var in RENDERING_VARS {
            if let Some(value) = env::var_os(var) {
                write!(
                    f,
                    "\nnote: {var}={} is set and changes rendering",
                    value.to_string_lossy()
                )?;
            }
        }
        Ok(())
    }
}

fn validate(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && !name.starts_with("failures/")
        && name.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
        });
    anyhow::ensure!(
        valid,
        "invalid golden name {name:?}: use letters, digits, '-', '_', '.' and '@', with '/' \
         between subfolders other than `failures`"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn screenshot(color: [u8; 4]) -> Screenshot {
        Screenshot {
            image: RgbaImage::from_pixel(4, 3, Rgba(color)),
            source: "test adapter".into(),
        }
    }

    fn goldens(test: &str) -> Goldens {
        let dir = env::temp_dir().join(format!("gpui-screenshot-{}-{test}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        Goldens::new(dir)
    }

    fn mismatch(result: Result<()>) -> Mismatch {
        result
            .expect_err("the screenshot must not match")
            .downcast::<Mismatch>()
            .expect("a mismatch, not an I/O error")
    }

    #[test]
    fn a_missing_golden_fails_and_writes_the_actual_image() {
        let goldens = goldens("missing");
        let shot = screenshot([10, 20, 30, 255]);
        let mismatch = mismatch(goldens.check_with("button", &shot, false));
        assert!(mismatch.difference.is_none());
        assert!(mismatch.actual.is_file());
        assert!(mismatch.diff.is_none());
        assert!(!goldens.path("button").exists());
        assert!(goldens.dir().join("failures/.gitignore").is_file());
        assert!(mismatch.to_string().contains("has no golden"));
    }

    #[test]
    fn updating_writes_the_golden_and_clears_failures() {
        let goldens = goldens("update");
        let shot = screenshot([10, 20, 30, 255]);
        goldens.check_with("nested/button", &shot, false).ok();
        goldens.check_with("nested/button", &shot, true).unwrap();
        let golden = goldens.path("nested/button");
        let decoded = output::decode_png(&fs::read(&golden).unwrap()).unwrap();
        assert!(decoded.image == shot.image);
        assert_eq!(decoded.source.as_deref(), Some("test adapter"));
        assert!(
            !goldens
                .dir()
                .join("failures/nested/button.actual.png")
                .exists()
        );
        goldens.check_with("nested/button", &shot, false).unwrap();
    }

    #[test]
    fn a_different_image_fails_with_a_diff() {
        let goldens = goldens("different");
        goldens
            .check_with("button", &screenshot([10, 20, 30, 255]), true)
            .unwrap();
        let mismatch =
            mismatch(goldens.check_with("button", &screenshot([10, 22, 30, 255]), false));
        let difference = mismatch.difference.unwrap();
        assert_eq!((difference.changed, difference.max_channel), (12, 2));
        assert!(mismatch.diff.as_ref().is_some_and(|diff| diff.is_file()));
        assert_eq!(mismatch.golden_source.as_deref(), Some("test adapter"));
        let tolerant = goldens.tolerance(Tolerance::channel(2));
        tolerant
            .check_with("button", &screenshot([10, 22, 30, 255]), false)
            .unwrap();
        assert!(!mismatch.actual.exists(), "a match clears earlier failures");
    }

    #[test]
    fn updating_leaves_a_matching_golden_alone() {
        let goldens = goldens("unchanged").tolerance(Tolerance::channel(2));
        goldens
            .check_with("button", &screenshot([10, 20, 30, 255]), true)
            .unwrap();
        let before = fs::read(goldens.path("button")).unwrap();
        goldens
            .check_with("button", &screenshot([10, 21, 30, 255]), true)
            .unwrap();
        assert_eq!(fs::read(goldens.path("button")).unwrap(), before);
    }

    #[test]
    fn names_stay_inside_the_folder() {
        let goldens = goldens("names");
        let shot = screenshot([0, 0, 0, 255]);
        for name in ["", "../x", "a//b", ".hidden", "failures/x", "a b", "/abs"] {
            let error = goldens.check_with(name, &shot, true).unwrap_err();
            assert!(error.downcast_ref::<Mismatch>().is_none(), "{name:?}");
        }
    }
}
