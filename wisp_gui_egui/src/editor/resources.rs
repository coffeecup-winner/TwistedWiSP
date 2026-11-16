// SPDX-License-Identifier: MIT OR Apache-2.0
//! One decode job at a time; cache and previews are independent of mutable DSP storage.
use super::{
    commands::{EditorCommand, RequestId},
    document::{DocumentId, EditorDocument, EditorNodeId},
};
use crossbeam::channel::{Receiver, Sender, bounded};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BufferView {
    pub status: String,
    pub path: Option<PathBuf>,
    pub preview: Arc<[(f32, f32)]>,
    pub frames: usize,
    pub rate: u32,
    pub channels: u16,
}
#[derive(Clone, Debug)]
pub struct BufferBinding {
    pub call: u32,
    pub audio: Arc<wisp::buffer::DecodedAudio>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    document: DocumentId,
    name: String,
    path: Option<PathBuf>,
}
struct Job {
    revision: u64,
    key: Key,
    relink: Option<(EditorNodeId, RequestId)>,
}
struct Completion {
    job: Job,
    audio: Result<Arc<wisp::buffer::DecodedAudio>, String>,
}
pub struct Resources {
    tx: Sender<Job>,
    rx: Receiver<Completion>,
    cache: BTreeMap<Key, Result<Arc<wisp::buffer::DecodedAudio>, String>>,
    active: Option<Key>,
    pending_relink: Option<Key>,
}
impl Default for Resources {
    fn default() -> Self {
        let (tx, jobs) = bounded::<Job>(1);
        let (results, rx) = bounded(1);
        std::thread::spawn(move || {
            while let Ok(job) = jobs.recv() {
                let audio = match &job.key.path {
                    Some(p) => wisp::buffer::DecodedAudio::load(p),
                    None => wisp::buffer::DecodedAudio::builtin(&job.key.name),
                }
                .map(Arc::new);
                if results.send(Completion { job, audio }).is_err() {
                    break;
                }
            }
        });
        Self {
            tx,
            rx,
            cache: BTreeMap::new(),
            active: None,
            pending_relink: None,
        }
    }
}
impl Resources {
    fn key(doc: &EditorDocument, node: EditorNodeId) -> Result<Key, String> {
        let s = doc.snapshot();
        let n = &s
            .nodes
            .iter()
            .find(|(id, _)| *id == node)
            .ok_or("Deleted buffer node")?
            .1;
        if n.function_name != "buffer" {
            return Err("Not a buffer node".into());
        }
        let name = n
            .buffer_name
            .clone()
            .ok_or("Choose a WAV or builtin sine")?;
        let reference = s
            .buffers
            .iter()
            .find(|r| r.name == name)
            .ok_or("Buffer reference not found")?;
        if let Some(error) = &reference.diagnostic {
            return Err(error.clone());
        }
        Ok(Key {
            document: s.id,
            name,
            path: reference.resolved.clone(),
        })
    }
    fn submit(&mut self, job: Job) -> Result<(), String> {
        if self.active.is_some() {
            return Err("Decoder busy; retry shortly".into());
        }
        let key = job.key.clone();
        self.tx.try_send(job).map_err(|_| "Decoder unavailable")?;
        self.active = Some(key);
        Ok(())
    }
    pub fn relink(
        &mut self,
        doc: &EditorDocument,
        node: EditorNodeId,
        path: Option<PathBuf>,
        request: RequestId,
    ) -> Result<(), String> {
        if !doc
            .snapshot()
            .nodes
            .iter()
            .any(|(id, n)| *id == node && n.function_name == "buffer")
        {
            return Err("Deleted buffer node".into());
        }
        let path = path
            .map(|p| p.canonicalize().map_err(|e| e.to_string()))
            .transpose()?;
        let name = if path.is_some() {
            format!("buffer_{}", node.0)
        } else {
            "sine".into()
        };
        self.submit(Job {
            revision: doc.binding_revision(),
            key: Key {
                document: doc.id(),
                name,
                path,
            },
            relink: Some((node, request)),
        })
    }
    pub fn poll(
        &mut self,
        document: Option<&EditorDocument>,
    ) -> Vec<Result<EditorCommand, (RequestId, String)>> {
        let mut commands = vec![];
        while let Ok(mut c) = self.rx.try_recv() {
            self.active = None;
            let valid = document.is_some_and(|d| {
                d.id() == c.job.key.document
                    && c.job.relink.is_none_or(|(id, _)| {
                        if d.binding_revision() != c.job.revision {
                            return false;
                        }
                        d.snapshot()
                            .nodes
                            .iter()
                            .any(|(n, v)| *n == id && v.function_name == "buffer")
                    })
            });
            if !valid {
                if let Some((_, r)) = c.job.relink {
                    commands.push(Err((r, "Stale buffer completion".into())));
                }
                continue;
            }
            let bytes = self
                .cache
                .iter()
                .filter(|(key, _)| **key != c.job.key)
                .filter_map(|(_, v)| v.as_ref().ok())
                .map(|a| a.samples.len() * 4)
                .sum::<usize>();
            if c.audio
                .as_ref()
                .is_ok_and(|a| bytes + a.samples.len() * 4 > 128 * 1024 * 1024)
            {
                c.audio = Err("Buffer cache limit (128 MiB); replace/close the document".into());
            }
            if let Some((node, req_id)) = c.job.relink {
                if c.audio.is_ok() {
                    self.pending_relink = Some(c.job.key.clone());
                }
                match &c.audio {
                    Ok(_) => commands.push(Ok(EditorCommand::ApplyBuffer {
                        document_id: c.job.key.document,
                        binding_revision: c.job.revision,
                        node_id: node,
                        name: c.job.key.name.clone(),
                        path: c.job.key.path.clone(),
                        req_id,
                    })),
                    Err(e) => commands.push(Err((req_id, e.clone()))),
                }
            }
            // A failed reload preserves the previous safe decoded cache entry.
            if c.audio.is_ok() || !self.cache.contains_key(&c.job.key) {
                self.cache.insert(c.job.key, c.audio);
            }
        }
        commands
    }
    pub fn sync(&mut self, doc: &EditorDocument) -> BTreeMap<EditorNodeId, BufferView> {
        let live_keys = doc
            .snapshot()
            .nodes
            .iter()
            .filter_map(|(id, n)| {
                (n.function_name == "buffer")
                    .then(|| Self::key(doc, *id).ok())
                    .flatten()
            })
            .collect::<std::collections::BTreeSet<_>>();
        if self
            .pending_relink
            .as_ref()
            .is_some_and(|key| live_keys.contains(key))
        {
            self.pending_relink = None;
        }
        self.cache.retain(|k, _| {
            k.document == doc.id()
                && (live_keys.contains(k) || self.pending_relink.as_ref() == Some(k))
        });
        if self
            .pending_relink
            .as_ref()
            .is_some_and(|key| key.document != doc.id())
        {
            self.pending_relink = None;
        }
        let mut views = BTreeMap::new();
        for (id, n) in doc
            .snapshot()
            .nodes
            .iter()
            .filter(|(_, n)| n.function_name == "buffer")
            .take(64)
        {
            let mut view = BufferView::default();
            match Self::key(doc, *id) {
                Err(e) => {
                    view.status = e;
                    view.path = doc.buffer_retry_path(*id);
                }
                Ok(key) => {
                    view.path = key.path.clone();
                    match self.cache.get(&key) {
                        Some(Ok(audio)) => {
                            view.status = if audio.sample_rate == 0 {
                                "Builtin single cycle".into()
                            } else {
                                format!(
                                    "{} Hz · {} channels · {:.3}s",
                                    audio.sample_rate,
                                    audio.channels,
                                    audio.samples.len() as f64 / audio.sample_rate as f64
                                )
                            };
                            view.frames = audio.samples.len();
                            view.rate = audio.sample_rate;
                            view.channels = audio.channels;
                            view.preview = audio.preview.clone();
                        }
                        Some(Err(e)) => view.status = e.clone(),
                        None => {
                            view.status = "Loading…".into();
                            if self.active.is_none() {
                                let _ = self.submit(Job {
                                    revision: doc.binding_revision(),
                                    key,
                                    relink: None,
                                });
                            }
                        }
                    }
                }
            }
            let _ = n;
            views.insert(*id, view);
        }
        views
    }
    pub fn bindings(&self, doc: &EditorDocument) -> Result<Vec<BufferBinding>, String> {
        let mut result = vec![];
        for (id, n) in &doc.snapshot().nodes {
            if n.function_name != "buffer" {
                continue;
            }
            if result.len() >= 64 {
                return Err("Buffer node limit (64)".into());
            }
            let key = Self::key(doc, *id)?;
            let audio = self
                .cache
                .get(&key)
                .ok_or("Buffer loading; retry Start shortly")?
                .as_ref()
                .map_err(Clone::clone)?
                .clone();
            result.push(BufferBinding {
                call: doc.runtime_call_index(*id)?,
                audio,
            });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{document::Edit, persistence::tests::Temp};
    use super::*;
    fn wait(
        manager: &mut Resources,
        doc: &EditorDocument,
    ) -> Vec<Result<EditorCommand, (RequestId, String)>> {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let result = manager.poll(Some(doc));
            if manager.active.is_none() {
                return result;
            }
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    fn wave(value: i16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend(38u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(22050u32.to_le_bytes());
        bytes.extend(44100u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(2u32.to_le_bytes());
        bytes.extend(value.to_le_bytes());
        bytes
    }
    #[test]
    fn duplicate_basenames_reload_failure_and_stale_decode() {
        let dir = Temp::new();
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wisp/core");
        let mut doc = EditorDocument::from_text(
            "[flow]\nname='media'\n[[flow.nodes]]\ntext='buffer'\n[[flow.nodes]]\ntext='buffer'\n",
            &core,
            DocumentId(1),
            None,
        )
        .unwrap();
        let ids = doc
            .snapshot()
            .nodes
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let mut manager = Resources::default();
        for (i, id) in ids.iter().enumerate() {
            let sub = dir.0.join(i.to_string());
            std::fs::create_dir(&sub).unwrap();
            let path = sub.join("same.wav");
            std::fs::write(&path, wave(if i == 0 { 16384 } else { 8192 })).unwrap();
            manager
                .relink(&doc, *id, Some(path), RequestId(i as u64))
                .unwrap();
            let mut commands = wait(&mut manager, &doc);
            assert_eq!(commands.len(), 1);
            let EditorCommand::ApplyBuffer {
                node_id,
                name,
                path,
                ..
            } = commands.remove(0).unwrap()
            else {
                panic!()
            };
            doc.edit(
                doc.snapshot().revision,
                Edit::Buffer {
                    node: node_id,
                    name,
                    path,
                },
            )
            .unwrap();
            manager.sync(&doc);
        }
        let bindings = manager.bindings(&doc).unwrap();
        assert_eq!(bindings[0].audio.samples[0], 0.5);
        assert_eq!(bindings[1].audio.samples[0], 0.25);
        let path = doc.buffer_retry_path(ids[0]).unwrap();
        let original = doc.serialize();
        std::fs::write(&path, b"bad wave").unwrap();
        manager
            .relink(&doc, ids[0], Some(path), RequestId(3))
            .unwrap();
        assert!(wait(&mut manager, &doc).remove(0).is_err());
        assert_eq!(doc.serialize(), original);
        assert_eq!(manager.bindings(&doc).unwrap()[0].audio.samples[0], 0.5);
        manager.relink(&doc, ids[0], None, RequestId(4)).unwrap();
        doc.edit(doc.snapshot().revision, Edit::Delete(vec![ids[0]]))
            .unwrap();
        assert!(wait(&mut manager, &doc).remove(0).is_err());
        assert_eq!(manager.bindings(&doc).unwrap().len(), 1);
    }
    #[test]
    fn builtin_sources_load_without_devices() {
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../wisp/core");
        let doc = EditorDocument::load(&core.join("../../wisp_gui/sine.twf"), &core, DocumentId(2))
            .unwrap();
        let mut manager = Resources::default();
        manager.sync(&doc);
        wait(&mut manager, &doc);
        let bindings = manager.bindings(&doc).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].audio.samples.len(), 1024);
        assert!((bindings[0].audio.samples[256] - 1.).abs() < 1e-6);
    }
}
