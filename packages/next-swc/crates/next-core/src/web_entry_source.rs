use anyhow::{anyhow, Result};
use turbo_binding::turbo::tasks_env::ProcessEnvVc;
use turbo_binding::turbo::tasks_fs::FileSystemPathVc;
use turbo_binding::turbopack::core::{
    chunk::{ChunkGroupVc, ChunkableAsset, ChunkableAssetVc, EvaluatedEntriesVc},
    reference_type::{EntryReferenceSubType, ReferenceType},
    resolve::{origin::PlainResolveOriginVc, parse::RequestVc},
};
use turbo_binding::turbopack::dev_server::{
    html::DevHtmlAssetVc,
    source::{asset_graph::AssetGraphContentSourceVc, ContentSourceVc},
};
use turbo_binding::turbopack::node::execution_context::ExecutionContextVc;
use turbo_binding::turbopack::turbopack::ecmascript::EcmascriptModuleAssetVc;
use turbo_tasks::{TryJoinIterExt, Value};

use crate::{
    next_client::context::{
        get_client_asset_context, get_client_chunking_context, get_client_compile_time_info,
        get_client_runtime_entries, ClientContextType,
    },
    next_config::NextConfigVc,
};

#[turbo_tasks::function]
pub async fn create_web_entry_source(
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    entry_requests: Vec<RequestVc>,
    server_root: FileSystemPathVc,
    env: ProcessEnvVc,
    eager_compile: bool,
    browserslist_query: &str,
    next_config: NextConfigVc,
) -> Result<ContentSourceVc> {
    let ty = Value::new(ClientContextType::Other);
    let compile_time_info = get_client_compile_time_info(browserslist_query);
    let context = get_client_asset_context(
        project_path,
        execution_context,
        compile_time_info,
        ty,
        next_config,
    );
    let chunking_context = get_client_chunking_context(
        project_path,
        server_root,
        compile_time_info.environment(),
        ty,
    );
    let entries = get_client_runtime_entries(project_path, env, ty, next_config, execution_context);

    let runtime_entries = entries.resolve_entries(context);

    let origin = PlainResolveOriginVc::new(context, project_path.join("_")).as_resolve_origin();
    let entries = entry_requests
        .into_iter()
        .map(|request| async move {
            let ty = Value::new(ReferenceType::Entry(EntryReferenceSubType::Web));
            Ok(origin
                .resolve_asset(request, origin.resolve_options(ty.clone()), ty)
                .primary_assets()
                .await?
                .first()
                .copied())
        })
        .try_join()
        .await?;

    let chunk_groups: Vec<_> = entries
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, module)| async move {
            if let Some(ecmascript) = EcmascriptModuleAssetVc::resolve_from(module).await? {
                let chunk_group = ChunkGroupVc::evaluated(
                    chunking_context,
                    ecmascript.into(),
                    if i == 0 {
                        runtime_entries
                    } else {
                        EvaluatedEntriesVc::empty()
                    },
                );
                Ok(chunk_group)
            } else if let Some(chunkable) = ChunkableAssetVc::resolve_from(module).await? {
                // TODO this is missing runtime code, so it's probably broken and we should also
                // add an ecmascript chunk with the runtime code
                Ok(ChunkGroupVc::from_chunk(
                    chunking_context,
                    chunkable.as_root_chunk(chunking_context),
                ))
            } else {
                // TODO convert into a serve-able asset
                Err(anyhow!(
                    "Entry module is not chunkable, so it can't be used to bootstrap the \
                     application"
                ))
            }
        })
        .try_join()
        .await?;

    let entry_asset = DevHtmlAssetVc::new(server_root.join("index.html"), chunk_groups).into();

    let graph = if eager_compile {
        AssetGraphContentSourceVc::new_eager(server_root, entry_asset)
    } else {
        AssetGraphContentSourceVc::new_lazy(server_root, entry_asset)
    }
    .into();
    Ok(graph)
}
