use anyhow::{bail, Result};
use turbo_binding::turbo::tasks_fs::FileSystemPathVc;
use turbo_binding::turbopack::core::{
    asset::Asset,
    chunk::{EvaluatedEntriesVc, EvaluatedEntryVc},
    context::AssetContextVc,
    resolve::{origin::PlainResolveOriginVc, parse::RequestVc},
};
use turbo_binding::turbopack::ecmascript::resolve::cjs_resolve;
use turbo_tasks::ValueToString;

#[turbo_tasks::value(shared)]
pub enum RuntimeEntry {
    Request(RequestVc, FileSystemPathVc),
    Evaluated(EvaluatedEntryVc),
}

#[turbo_tasks::value_impl]
impl RuntimeEntryVc {
    #[turbo_tasks::function]
    pub async fn resolve_entry(self, context: AssetContextVc) -> Result<EvaluatedEntriesVc> {
        let (request, path) = match *self.await? {
            RuntimeEntry::Evaluated(e) => return Ok(EvaluatedEntriesVc::cell(vec![e])),
            RuntimeEntry::Request(r, path) => (r, path),
        };

        let assets = cjs_resolve(PlainResolveOriginVc::new(context, path).into(), request)
            .primary_assets()
            .await?;

        let mut runtime_entries = Vec::with_capacity(assets.len());
        for asset in &assets {
            if let Some(entry) = EvaluatedEntryVc::resolve_from(asset).await? {
                runtime_entries.push(entry);
            } else {
                bail!(
                    "runtime reference resolved to an asset ({}) that cannot be evaluated",
                    asset.ident().to_string().await?
                );
            }
        }

        Ok(EvaluatedEntriesVc::cell(runtime_entries))
    }
}

#[turbo_tasks::value(transparent)]
pub struct RuntimeEntries(Vec<RuntimeEntryVc>);

#[turbo_tasks::value_impl]
impl RuntimeEntriesVc {
    #[turbo_tasks::function]
    pub async fn resolve_entries(self, context: AssetContextVc) -> Result<EvaluatedEntriesVc> {
        let mut runtime_entries = Vec::new();

        for reference in &self.await? {
            let resolved_entries = reference.resolve_entry(context).await?;
            runtime_entries.extend(resolved_entries.into_iter());
        }

        Ok(EvaluatedEntriesVc::cell(runtime_entries))
    }
}
