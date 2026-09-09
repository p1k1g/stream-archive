from pathlib import Path


def patch(path, old, new, label):
    p=Path(path)
    s=p.read_text(encoding='utf-8')
    if old not in s:
        raise SystemExit(f'patch target not found: {label}')
    p.write_text(s.replace(old,new,1),encoding='utf-8',newline='\n')

idx='rust-web/web/index.html'
patch(idx,
'''    <p class="hint">저장된 값은 API/화면으로 다시 노출하지 않습니다. Windows CurrentUser DPAPI로 암호화하며 기존 dpapi:v1 값과 호환됩니다.</p>\n\n  <div id="p102AdvancedPanel" hidden>''',
'''    <p class="hint">저장된 값은 API/화면으로 다시 노출하지 않습니다. Windows CurrentUser DPAPI로 암호화하며 기존 dpapi:v1 값과 호환됩니다.</p>\n  </div>\n  <div id="p102AdvancedPanel" hidden>''',
'close security before advanced')
patch(idx,
'''    <p class="hint">일반 사용자는 이 영역을 변경할 필요가 없습니다. 기본 서버는 127.0.0.1:8787 로컬 전용이며, 외부 접속은 Caddy/HTTPS 구성을 권장합니다.</p>\n  </div>\n  </div>\n</section>''',
'''    <p class="hint">일반 사용자는 이 영역을 변경할 필요가 없습니다. 기본 서버는 127.0.0.1:8787 로컬 전용이며, 외부 접속은 Caddy/HTTPS 구성을 권장합니다.</p>\n  </div>\n</section>''',
'remove stale nested close')
# Bust cached HTML-linked assets so the follow-up is visible immediately after repackaging.
p=Path(idx); s=p.read_text(encoding='utf-8').replace('p10-2','p10-2b'); p.write_text(s,encoding='utf-8',newline='\n')

p10='rust-web/web/phase10.js'
patch(p10,
'#tokenBtn{flex:0 0 auto}',
'#tokenBtn{flex:0 0 auto;margin-left:12px}',
'recovery token spacing')

s=Path(idx).read_text(encoding='utf-8')
sec=s.index('<div id="p8SecurityPanel"')
adv=s.index('<div id="p102AdvancedPanel"')
close=s.index('</div>',sec)
# The first div close may be nested title/grid, so verify via the exact boundary introduced above too.
assert '</p>\n  </div>\n  <div id="p102AdvancedPanel"' in s
assert sec < adv
assert s.count('id="p102AdvancedPanel"') == 1
assert 'v=p10-2b' in s
assert 'margin-left:12px' in Path(p10).read_text(encoding='utf-8')
print('Phase 10.2 follow-up sanity: PASS')
