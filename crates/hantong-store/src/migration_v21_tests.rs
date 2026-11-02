fn seed_v20_fixture(f:&Fixture){seed_v19_fixture(f);Store{connection:f.keyed()}.migrate_v19_to_v20().unwrap();}
fn v20_product_rows(c:&rusqlite::Connection)->rusqlite::Result<V14TableRows>{
 frozen_product_table_names(SCHEMA_V20)?.into_iter().filter(|n|n!="app_metadata").map(|name|{
  let mut q=c.prepare(&format!("PRAGMA table_info({name})"))?;
  let columns=q.query_map([],|r|r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
  Ok((name.clone(),read_table_columns(c,&name,&columns)?))
 }).collect()
}
#[test]
fn sync07_mutation_v21_migration_preserves_every_table_and_issues_no_grant(){
 let f=Fixture::new();seed_v20_fixture(&f);let before=f.keyed().with_connection(v20_product_rows).unwrap();
 let store=Store{connection:f.keyed()};
 store.migrate_v20_to_v21().unwrap();
 store.migrate_v21_to_v22().unwrap();
 store.with_connection(|c|{
  assert_eq!(before,v20_product_rows(c)?);
  assert_eq!(c.query_row("SELECT count(*) FROM google_mutation_requests",[],|r|r.get::<_,i64>(0))?,0);
   validate_schema(c,SCHEMA_V22,22)
 }).unwrap();assert!(f.0.join("data/backup-before-v20.db").is_file());
}
#[test]
fn sync07_mutation_v21_backup_collision_and_validation_failure_preserve_old_database(){
 let f=Fixture::new();seed_v20_fixture(&f);let path=f.0.join("data/mail-hantong.db");let before=fs::read(&path).unwrap();
 let backup=f.0.join("data/backup-before-v20.db");fs::write(&backup,b"synthetic reserved backup").unwrap();
 assert!(matches!(f.open(),Err(ErrorCode::Conflict)));assert_eq!(fs::read(&path).unwrap(),before);assert_eq!(fs::read(backup).unwrap(),b"synthetic reserved backup");
 let g=Fixture::new();seed_v20_fixture(&g);let s=Store{connection:g.keyed()};let before=s.with_connection(v20_product_rows).unwrap();
 assert_eq!(s.migrate_with_backup(20,21,SCHEMA,|c|{c.execute_batch(MIGRATION_V20_TO_V21)?;c.execute_batch("CREATE TABLE unexpected_mutation_schema(value)")}),Err(ErrorCode::InternalError));
 s.with_connection(|c|{assert_eq!(before,v20_product_rows(c)?);validate_schema(c,SCHEMA_V20,20)}).unwrap();
}
