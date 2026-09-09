from pathlib import Path
p=Path('rust-web/src/vod_queue.rs')
s=p.read_text(encoding='utf-8')
old='''        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM vod_queue WHERE state IN ('QUEUED','STARTING','RUNNING','CANCELLING')",
            [],
            |row| row.get(0),
        )?;
        if pending >= QUEUE_LIMIT as i64 {
            bail!("VOD 다운로드 큐는 실행/대기 작업을 최대 {QUEUE_LIMIT}건까지 보관합니다.");
        }
        tx.execute(
            r#"INSERT INTO vod_queue(id,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
               VALUES(?1,?2,?3,?4,'QUEUED',0,'대기 중',?5,?5)"#,
            params![id, request_json, req.vod_url, req.output_directory, now],
        )?;
        tx.commit()?;
        drop(conn);
'''
new='''        {
            let mut conn = self.conn()?;
            let tx = conn.transaction()?;
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM vod_queue WHERE state IN ('QUEUED','STARTING','RUNNING','CANCELLING')",
                [],
                |row| row.get(0),
            )?;
            if pending >= QUEUE_LIMIT as i64 {
                bail!("VOD 다운로드 큐는 실행/대기 작업을 최대 {QUEUE_LIMIT}건까지 보관합니다.");
            }
            tx.execute(
                r#"INSERT INTO vod_queue(id,request_json,vod_url,output_directory,state,attempts,message,created_at,updated_at)
                   VALUES(?1,?2,?3,?4,'QUEUED',0,'대기 중',?5,?5)"#,
                params![id, request_json, req.vod_url, req.output_directory, now],
            )?;
            tx.commit()?;
        }
'''
if old not in s:
    raise SystemExit('enqueue Send-boundary target not found')
p.write_text(s.replace(old,new,1),encoding='utf-8',newline='\n')
print('enqueue Send boundary fixed')
