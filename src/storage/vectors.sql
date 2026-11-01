create table meta(id integer primary key check(id=1), config_identity text not null);
create table vector_keys(row_id integer primary key, search_hash text not null unique);
create virtual table vector_index using vec1(vector);
create table query_error(id integer primary key check(id=1), at integer not null, error text not null);
create table embed_failures(search_hash text primary key, attempts integer not null check(attempts>=1), error text);
insert into vector_index(cmd,arg) values('rebuild','{index:"none",distance:"cos"}');
