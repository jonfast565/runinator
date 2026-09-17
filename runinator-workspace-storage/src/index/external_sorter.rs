#[allow(unused_imports)]
use super::*;

pub struct ExternalSorter {
    pub(super) dir: TempDir,
    pub(super) buffer: Vec<Location>,
    pub(super) runs: u64,
}

impl ExternalSorter {
    pub fn new(dir: &Path) -> Result<Self> {
        Ok(Self {
            dir: TempDir::new_in(dir)?,
            buffer: Vec::with_capacity(SORT_RUN_ENTRIES),
            runs: 0,
        })
    }
    pub(super) fn path(&self, round: u64, run: u64) -> PathBuf {
        self.dir.path().join(format!("{round}-{run}"))
    }
    pub fn push(&mut self, e: Location) -> Result<()> {
        self.buffer.push(e);
        if self.buffer.len() == SORT_RUN_ENTRIES {
            self.flush()?;
        }
        Ok(())
    }
    pub(super) fn flush(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        self.buffer.sort_unstable_by_key(|e| e.id);
        let mut out = BufWriter::new(File::create(self.path(0, self.runs))?);
        for e in self.buffer.drain(..) {
            out.write_all(&e.bytes())?;
        }
        self.runs += 1;
        Ok(())
    }
    pub fn finish(mut self, pack: Id, output_dir: &Path) -> Result<NamedTempFile> {
        self.flush()?;
        let mut round = 0;
        let mut count = self.runs;
        while count > 1 {
            let jobs = (0..count)
                .step_by(2)
                .enumerate()
                .map(|(output, run)| {
                    (
                        self.path(round, run),
                        (run + 1 < count).then(|| self.path(round, run + 1)),
                        self.path(round + 1, output as u64),
                    )
                })
                .collect::<Vec<_>>();
            jobs.par_iter()
                .try_for_each(|(a, b, target)| -> Result<()> {
                    let Some(b) = b else {
                        fs::rename(a, target)?;
                        return Ok(());
                    };
                    merge_raw(a, b, target)?;
                    fs::remove_file(a)?;
                    fs::remove_file(b)?;
                    Ok(())
                })?;
            count = jobs.len() as u64;
            round += 1;
        }
        let mut out = IndexWriter::new(output_dir)?;
        if count == 1 {
            let mut input = BufReader::new(File::open(self.path(round, 0))?);
            while let Some(mut e) = next_raw(&mut input)? {
                e.pack = pack;
                out.push(e)?;
            }
        }
        out.finish()
    }
}
