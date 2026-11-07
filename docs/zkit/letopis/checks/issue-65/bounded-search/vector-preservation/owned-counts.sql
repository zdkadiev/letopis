.load /Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/oracle-vec1.dylib sqlite3_extension_init
select count(*) as native_vectors, sum(length(vector)) as vector_bytes from vector_index;
