//! The embedding model: download once, then run with tract.
//!
//! bge-small-en-v1.5 takes `input_ids`, `attention_mask` and (optionally)
//! `token_type_ids`, and returns the last hidden state `[1, seq, 384]`; the
//! sentence vector is the first token's (CLS) row, L2-normalised.

use super::{Embedder, Status, DIM, MODEL_ID};
use crate::state::App;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tract_onnx::prelude::*;

/// Where the files come from. Pinned to the repository's main branch.
const BASE: &str = "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main";
const FILES: &[(&str, &str)] = &[("onnx/model.onnx", "model.onnx"), ("tokenizer.json", "tokenizer.json")];
/// Longest input, in tokens; longer text is cut (titles and the start of a body are plenty).
const MAX_TOKENS: usize = 256;

pub fn dir(app: &App) -> PathBuf {
    app.data_dir.join("models").join(MODEL_ID)
}

/// Download any missing file, then load the model and install it.
pub fn ensure(app: &App) -> anyhow::Result<()> {
    let d = dir(app);
    std::fs::create_dir_all(&d)?;
    for (remote, local) in FILES {
        let path = d.join(local);
        if path.exists() {
            continue;
        }
        download(app, &format!("{BASE}/{remote}"), &path)?;
    }
    app.embed.set(Status::Loading);
    let e = Onnx::load(&d)?;
    app.embed.install(std::sync::Arc::new(e));
    Ok(())
}

fn download(app: &App, url: &str, to: &Path) -> anyhow::Result<()> {
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .build()
        .new_agent();
    let mut resp = agent.get(url).call().map_err(|e| anyhow::anyhow!("couldn't reach Hugging Face for the embedding model ({e}). The Mac needs to reach huggingface.co once; retry from Settings → AI."))?;
    if !resp.status().is_success() {
        anyhow::bail!("downloading {url}: HTTP {}", resp.status());
    }
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok()?.parse().ok());
    let part = to.with_extension("part");
    let mut out = std::fs::File::create(&part)?;
    let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        app.embed.set(Status::Downloading { done, total });
    }
    out.sync_all()?;
    if total.is_some_and(|t| t != done) {
        anyhow::bail!("downloading {url}: got {done} of {} bytes", total.unwrap_or(0));
    }
    std::fs::rename(&part, to)?;
    Ok(())
}

type Plan = TypedRunnableModel;

pub struct Onnx {
    plan: std::sync::Arc<Plan>,
    tokenizer: tokenizers::Tokenizer,
    /// Model input order as (name) → which of ids / mask / types it is.
    inputs: Vec<Input>,
}

#[derive(Clone, Copy)]
enum Input {
    Ids,
    Mask,
    Types,
}

impl Onnx {
    pub fn load(dir: &Path) -> anyhow::Result<Onnx> {
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("tokenizer.json: {e}"))?;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: MAX_TOKENS,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!("tokenizer: {e}"))?;
        tokenizer.with_padding(None);
        let mut model = tract_onnx::onnx().model_for_path(dir.join("model.onnx"))?;
        let s = model.sym("S");
        let mut inputs = vec![];
        for i in 0..model.input_outlets()?.len() {
            let name = model.node(model.input_outlets()?[i].node).name.clone();
            let which = match name.as_str() {
                n if n.contains("input_ids") => Input::Ids,
                n if n.contains("attention_mask") => Input::Mask,
                n if n.contains("token_type") => Input::Types,
                other => anyhow::bail!("unexpected model input '{other}'"),
            };
            inputs.push(which);
            model = model.with_input_fact(i, i64::fact(tract_onnx::tract_core::dims!(1, s)).into())?;
        }
        let plan = model.into_optimized()?.into_runnable()?;
        Ok(Onnx {
            plan,
            tokenizer,
            inputs,
        })
    }

    fn one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;
        let ids: Vec<i64> = enc.get_ids().iter().map(|&x| x as i64).collect();
        let n = ids.len().max(1);
        let mask: Vec<i64> = enc.get_attention_mask().iter().map(|&x| x as i64).collect();
        let types: Vec<i64> = vec![0; n];
        let t = |v: &[i64]| Tensor::from_shape(&[1, n], v);
        let mut feed: TVec<TValue> = tvec![];
        for which in &self.inputs {
            feed.push(
                match which {
                    Input::Ids => t(&ids)?,
                    Input::Mask => t(&mask)?,
                    Input::Types => t(&types)?,
                }
                .into(),
            );
        }
        let out = self.plan.run(feed)?;
        let hidden = out[0].to_plain_array_view::<f32>()?;
        let shape = hidden.shape().to_vec();
        let cls: Vec<f32> = match shape.len() {
            3 => (0..shape[2]).map(|j| hidden[[0, 0, j]]).collect(),
            2 => (0..shape[1]).map(|j| hidden[[0, j]]).collect(),
            _ => anyhow::bail!("unexpected model output shape {shape:?}"),
        };
        if cls.len() != DIM {
            anyhow::bail!("model gives {} dimensions, expected {DIM}", cls.len());
        }
        let norm = cls.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        Ok(cls.iter().map(|x| x / norm).collect())
    }
}

impl Embedder for Onnx {
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.one(t)).collect()
    }
    fn id(&self) -> &str {
        MODEL_ID
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny stand-in with the real model's interface (inputs, output
    /// shape, tokenizer format), generated by tests/fixtures/embed/make.py.
    #[test]
    fn runs_an_onnx_model_through_tract() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/embed");
        let e = Onnx::load(&dir).unwrap();
        let v = e
            .embed(&["cutover plan".into(), "cutover plan".into(), "dentist".into()])
            .unwrap();
        assert_eq!(v[0].len(), DIM);
        let norm: f32 = v[0].iter().map(|x| x * x).sum();
        assert!((norm - 1.0).abs() < 1e-4, "unit length ({norm})");
        assert!(
            (super::super::cosine(&v[0], &v[1]) - 1.0).abs() < 1e-5,
            "same text, same vector"
        );
        assert!(super::super::cosine(&v[0], &v[2]) < 0.999);
        let long = "word ".repeat(2000);
        e.embed(&[long]).expect("long text is cut, not an error");
    }
}
