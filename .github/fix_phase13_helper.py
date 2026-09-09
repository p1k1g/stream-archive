from pathlib import Path
p=Path('.github/phase13_codex_fix.py')
s=p.read_text(encoding='utf-8')
old='        let err = queue.enqueue(request("C:\\\\SOOP_VOD")).await.unwrap_err();'
new='        let err = queue.enqueue(request("C:/SOOP_VOD")).await.unwrap_err();'
if old not in s:
    raise SystemExit('escape-fix target not found')
p.write_text(s.replace(old,new,1),encoding='utf-8',newline='\n')
print('helper escaping fixed')
