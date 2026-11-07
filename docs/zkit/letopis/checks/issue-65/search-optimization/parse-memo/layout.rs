extern crate letopis;
fn main() { println!("ParsedElement={} ParsedFile={} Element={} RepoPath={}", std::mem::size_of::<letopis::service::ParsedElement>(),std::mem::size_of::<letopis::service::ParsedFile>(),std::mem::size_of::<letopis::domain::Element>(),std::mem::size_of::<letopis::domain::RepoPath>()); }
