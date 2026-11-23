//! The local Scorer: a small open embedding model run on the CPU, no key.
//!
//! `synthetic-customers setup` downloads the model once from Hugging Face
//! into the user cache folder. Scoring then only reads that folder and never
//! touches the network. The ONNX model runs through tract, a pure-Rust ONNX
//! runtime, so the binary links no native library. See `docs/scorer.md` for
//! why this model was chosen and how well it agrees with the gold set.

use crate::embedder::Embedder;
use anyhow::{Context, Result, bail};
use hf_hub::api::sync::ApiBuilder;
use hf_hub::{Cache, Repo, RepoType};
use std::path::{Path, PathBuf};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};
use tract_onnx::prelude::*;

/// Environment variable that overrides where models are cached.
pub const CACHE_DIR_ENV: &str = "SYNTHETIC_CUSTOMERS_CACHE_DIR";

/// How token vectors become one vector per text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pooling {
    /// Average of the token vectors, padding left out (sentence-transformers).
    Mean,
    /// The first token's vector (BGE, Arctic).
    Cls,
}

/// An embedding model the local Scorer can run, pinned to one revision.
#[derive(Debug, Clone, Copy)]
pub struct LocalModel {
    /// Short name shown in the report, e.g. `bge-small-en-v1.5`.
    pub name: &'static str,
    /// Hugging Face repository.
    pub repo: &'static str,
    /// Pinned commit, so every user scores with the same weights.
    pub revision: &'static str,
    /// The ONNX file inside the repository.
    pub onnx_file: &'static str,
    pub pooling: Pooling,
    /// Download size in MB, for the setup message.
    pub size_mb: u32,
}

const TOKENIZER_FILE: &str = "tokenizer.json";
/// Longest input in tokens; Synthetic customers answer in a few sentences.
const MAX_TOKENS: usize = 512;
/// Texts embedded per model call.
const BATCH: usize = 16;

/// The model the local Scorer uses. Chosen on the gold set; see
/// `docs/scorer.md`.
pub const DEFAULT_MODEL: LocalModel = CANDIDATES[0];

/// Every model compared on the gold set, the default first. Only the
/// default is used for scoring; the others stay here so the comparison can
/// be run again (`cargo run --release --example compare_scorers`).
pub const CANDIDATES: [LocalModel; 9] = [
    LocalModel {
        name: "bge-small-en-v1.5",
        repo: "Xenova/bge-small-en-v1.5",
        revision: "ea104dacec62c0de699686887e3f920caeb4f3e3",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Cls,
        size_mb: 134,
    },
    LocalModel {
        name: "all-MiniLM-L6-v2",
        repo: "Xenova/all-MiniLM-L6-v2",
        revision: "751bff37182d3f1213fa05d7196b954e230abad9",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Mean,
        size_mb: 91,
    },
    LocalModel {
        name: "all-MiniLM-L12-v2",
        repo: "Xenova/all-MiniLM-L12-v2",
        revision: "beeb2e4b69e95f188a15cc2e90d09fd035dac229",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Mean,
        size_mb: 134,
    },
    LocalModel {
        name: "gte-small",
        repo: "Xenova/gte-small",
        revision: "5927d1727bb12db490052a1b33265ad78058de08",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Mean,
        size_mb: 134,
    },
    LocalModel {
        name: "snowflake-arctic-embed-s",
        repo: "Snowflake/snowflake-arctic-embed-s",
        revision: "e596f507467533e48a2e17c007f0e1dacc837b33",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Cls,
        size_mb: 134,
    },
    LocalModel {
        name: "mxbai-embed-xsmall-v1",
        repo: "mixedbread-ai/mxbai-embed-xsmall-v1",
        revision: "e6ac24e5d6efb8782b59de1647b3ececb4ece94e",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Mean,
        size_mb: 97,
    },
    LocalModel {
        name: "all-mpnet-base-v2 (int8)",
        repo: "Xenova/all-mpnet-base-v2",
        revision: "e086c5e0b3a57b0ce46dd6d9c0662948860b35f3",
        onnx_file: "onnx/model_quantized.onnx",
        pooling: Pooling::Mean,
        size_mb: 111,
    },
    LocalModel {
        name: "bge-base-en-v1.5 (int8)",
        repo: "Xenova/bge-base-en-v1.5",
        revision: "4d6cd88e18e51a5e020c2c305726d76ada9c03cf",
        onnx_file: "onnx/model_quantized.onnx",
        pooling: Pooling::Cls,
        size_mb: 111,
    },
    LocalModel {
        name: "all-mpnet-base-v2",
        repo: "Xenova/all-mpnet-base-v2",
        revision: "e086c5e0b3a57b0ce46dd6d9c0662948860b35f3",
        onnx_file: "onnx/model.onnx",
        pooling: Pooling::Mean,
        size_mb: 436,
    },
];

/// The folder models are cached in: `$SYNTHETIC_CUSTOMERS_CACHE_DIR`, or
/// `synthetic-customers/models` in the user cache folder (`~/Library/Caches`
/// on macOS, `~/.cache` on Linux, `%LOCALAPPDATA%` on Windows).
pub fn cache_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(CACHE_DIR_ENV).filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let base = dirs::cache_dir().context(
        "cannot find the user cache folder; set SYNTHETIC_CUSTOMERS_CACHE_DIR to a folder \
         for the scoring model",
    )?;
    Ok(base.join("synthetic-customers").join("models"))
}

impl LocalModel {
    fn hub_repo(&self) -> Repo {
        Repo::with_revision(
            self.repo.to_string(),
            RepoType::Model,
            self.revision.to_string(),
        )
    }

    fn files(&self) -> [&'static str; 2] {
        [TOKENIZER_FILE, self.onnx_file]
    }

    /// The cached model files, without touching the network; `None` when
    /// any is missing.
    pub fn cached_files(&self, cache: &Path) -> Option<[PathBuf; 2]> {
        let repo = Cache::new(cache.to_path_buf()).repo(self.hub_repo());
        let [tokenizer, onnx] = self.files().map(|f| repo.get(f));
        Some([tokenizer?, onnx?])
    }

    /// Download the model into `cache` unless it is already there. Returns
    /// true when something was downloaded.
    pub fn download(&self, cache: &Path) -> Result<bool> {
        if self.cached_files(cache).is_some() {
            return Ok(false);
        }
        std::fs::create_dir_all(cache)
            .with_context(|| format!("cannot create the model cache {}", cache.display()))?;
        let api = ApiBuilder::from_cache(Cache::new(cache.to_path_buf()))
            .with_progress(true)
            .build()
            .context("cannot set up the Hugging Face download")?;
        let repo = api.repo(self.hub_repo());
        for file in self.files() {
            repo.get(file).with_context(|| {
                format!(
                    "cannot download {file} of {} from huggingface.co (check the network)",
                    self.repo
                )
            })?;
        }
        Ok(true)
    }

    /// Delete this model's files from `cache`.
    pub fn remove(&self, cache: &Path) -> Result<()> {
        let dir = cache.join(format!("models--{}", self.repo.replace('/', "--")));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .with_context(|| format!("cannot delete {}", dir.display()))?;
        }
        Ok(())
    }
}

type Plan = Arc<TypedRunnableModel>;

/// Which tensor feeds each model input.
#[derive(Debug, Clone, Copy)]
enum Input {
    Ids,
    Mask,
    TypeIds,
}

/// The local Scorer's Embedder, loaded from the cache.
pub struct LocalEmbedder {
    model: LocalModel,
    /// `local (<model>)`, shown in the report.
    display_name: String,
    tokenizer: Tokenizer,
    plan: Plan,
    inputs: Vec<Input>,
}

impl LocalEmbedder {
    /// Load `model` from `cache`. Never downloads: when the model is not
    /// cached, the error says to run `synthetic-customers setup`.
    pub fn load(model: LocalModel, cache: &Path) -> Result<Self> {
        let Some([tokenizer_path, onnx_path]) = model.cached_files(cache) else {
            bail!(
                "the local Scorer's model ({}) is not downloaded yet: run \
                 `synthetic-customers setup` once (needs the network, about {} MB), \
                 then score offline",
                model.name,
                model.size_mb
            );
        };
        let mut tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("cannot read {}", tokenizer_path.display()))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKENS,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        tokenizer.with_padding(Some(PaddingParams::default()));

        let (plan, inputs) = load_onnx(&onnx_path)
            .with_context(|| format!("cannot load the ONNX model {}", onnx_path.display()))?;
        Ok(Self {
            model,
            display_name: format!("local ({})", model.name),
            tokenizer,
            plan,
            inputs,
        })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("cannot tokenize: {e}"))?;
        let batch = encodings.len();
        let len = encodings.iter().map(|e| e.len()).max().unwrap_or(0);
        let column = |f: &dyn Fn(&tokenizers::Encoding) -> &[u32]| -> Result<Tensor> {
            let data: Vec<i64> = encodings
                .iter()
                .flat_map(|e| f(e).iter().map(|&x| x as i64))
                .collect();
            Ok(tract_ndarray::Array2::from_shape_vec((batch, len), data)?.into())
        };
        let ids = column(&|e| e.get_ids())?;
        let mask = column(&|e| e.get_attention_mask())?;
        let type_ids = column(&|e| e.get_type_ids())?;
        let inputs: TVec<TValue> = self
            .inputs
            .iter()
            .map(|i| match i {
                Input::Ids => ids.clone(),
                Input::Mask => mask.clone(),
                Input::TypeIds => type_ids.clone(),
            })
            .map(TValue::from)
            .collect();
        let outputs = self.plan.run(inputs)?;
        let hidden = outputs[0].to_plain_array_view::<f32>()?;
        let shape = hidden.shape().to_vec();
        if shape.len() != 3 || shape[0] != batch {
            bail!("unexpected model output shape {shape:?}");
        }
        let mut vectors = Vec::with_capacity(batch);
        for (b, encoding) in encodings.iter().enumerate() {
            let mut v = vec![0.0f64; shape[2]];
            match self.model.pooling {
                Pooling::Cls => {
                    for (d, x) in v.iter_mut().enumerate() {
                        *x = hidden[[b, 0, d]] as f64;
                    }
                }
                Pooling::Mean => {
                    let mask = encoding.get_attention_mask();
                    let mut count = 0.0;
                    for (t, &m) in mask.iter().enumerate() {
                        if m == 0 {
                            continue;
                        }
                        count += 1.0;
                        for (d, x) in v.iter_mut().enumerate() {
                            *x += hidden[[b, t, d]] as f64;
                        }
                    }
                    if count > 0.0 {
                        v.iter_mut().for_each(|x| *x /= count);
                    }
                }
            }
            vectors.push(v);
        }
        Ok(vectors)
    }
}

/// Load an ONNX text encoder. Most exports name their batch and sequence
/// dimensions; for one that does not, they are declared here as symbols.
fn load_onnx(path: &Path) -> Result<(Plan, Vec<Input>)> {
    match load_onnx_with(path, false) {
        Ok(loaded) => Ok(loaded),
        Err(_) => load_onnx_with(path, true),
    }
}

fn load_onnx_with(path: &Path, declare_dims: bool) -> Result<(Plan, Vec<Input>)> {
    let mut model = tract_onnx::onnx().model_for_path(path)?;
    let batch = model.symbols.sym("B");
    let seq = model.symbols.sym("S");
    let mut inputs = Vec::new();
    for i in 0..model.inputs.len() {
        let name = model.node(model.inputs[i].node).name.clone();
        inputs.push(match name.as_str() {
            "input_ids" => Input::Ids,
            "attention_mask" => Input::Mask,
            "token_type_ids" => Input::TypeIds,
            other => bail!("the model has an unknown input `{other}`"),
        });
        if declare_dims {
            model.set_input_fact(
                i,
                InferenceFact::dt_shape(i64::datum_type(), tvec!(batch.to_dim(), seq.to_dim())),
            )?;
        }
    }
    let plan = model.into_optimized()?.into_runnable()?;
    Ok((plan, inputs))
}

impl Embedder for LocalEmbedder {
    fn name(&self) -> &str {
        &self.display_name
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f64>>> {
        let mut vectors = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(BATCH) {
            vectors.extend(self.embed_batch(chunk)?);
        }
        Ok(vectors)
    }
}
