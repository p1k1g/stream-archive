from pathlib import Path

p=Path('rust-web/src/realtime.rs')
s=p.read_text(encoding='utf-8')
s=s.replace('use crate::{ApiResult, AppState, backend::LogBuffer, internal_error};','use crate::{ApiResult, AppState, internal_error};\n#[cfg(test)]\nuse crate::backend::LogBuffer;')
s=s.replace('    response::sse::{Event, KeepAlive, Sse},','    response::{IntoResponse, sse::{Event, KeepAlive, Sse}},')
s=s.replace(') -> ApiResult<Sse<ReceiverStream<Result<Event, Infallible>>>> {',') -> ApiResult<impl IntoResponse> {')
s=s.replace('    let (sender, receiver) = mpsc::channel(8);','    let (sender, receiver) = mpsc::channel::<Result<Event, Infallible>>(8);')
p.write_text(s,encoding='utf-8',newline='\n')
print('Phase 11 compile fix applied')
