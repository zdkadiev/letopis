use super::{DocumentResult, FileSystem, KnowledgeMetadata, MarkdownCodec};
use crate::domain::RepoPath;

/// Compare a live whole-file projection with the selected pinned knowledge metadata.
pub(super) fn is_current_knowledge<F: FileSystem>(
    node: &KnowledgeMetadata,
    path: &RepoPath,
    filesystem: &F,
    markdown: &dyn MarkdownCodec,
) -> DocumentResult<bool> {
    let Some(file) = filesystem.read_file(path)? else {
        return Ok(false);
    };
    let Ok(Some(parsed)) = markdown.parse_file(path, &file.bytes) else {
        return Ok(false);
    };
    Ok(parsed.elements.iter().any(|current| {
        current.element.id == node.id
            && current.element.hash == node.hash
            && current.element.lines == node.lines
    }))
}
