//! Native Windows media session integration.
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Weak};
use crate::NotchWindow;
use std::time::Duration;
use notch_core::lyrics::{self, Line};

#[derive(Clone, Default)]
struct State { active: bool, title: String, artist: String, playing: bool, position: f64, length: f64 }
fn time_text(v:f64)->String{let s=v.max(0.0)as u64;format!("{}:{:02}",s/60,s%60)}
fn active_lyric(lines:&[Line],position:f64,length:f64,synced:bool)->i32{
 if lines.is_empty(){return -1;}
 if synced{return lines.iter().rposition(|line|line.at<=position).map(|i|i as i32).unwrap_or(-1);}
 if length<=0.0{return -1;}
 ((position.max(0.0)/length).clamp(0.0,0.999_999)*lines.len() as f64).floor() as i32
}

#[cfg(windows)]
mod platform {
 use super::*;
 use windows::Media::Control::{GlobalSystemMediaTransportControlsSession as Session,GlobalSystemMediaTransportControlsSessionManager as Manager,GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status};
 use windows::Storage::Streams::DataReader;
 use windows::Win32::System::WinRT::{RoInitialize,RO_INIT_MULTITHREADED};
 fn init(){unsafe{let _=RoInitialize(RO_INIT_MULTITHREADED);}}
 fn app_id(s:&Session)->String{s.SourceAppUserModelId().map(|x|x.to_string()).unwrap_or_default().to_lowercase()}
 fn is_self(s:&Session)->bool{let id=app_id(s);if id.contains("webview2"){return true;}std::env::current_exe().ok().and_then(|p|p.file_stem().map(|x|x.to_string_lossy().to_lowercase())).map(|n|n.len()>=3&&id.contains(&n)).unwrap_or(false)}
 fn session()->windows::core::Result<Session>{init();let m=Manager::RequestAsync()?.get()?;if let Ok(s)=m.GetCurrentSession(){if !is_self(&s){return Ok(s);}}for s in m.GetSessions()?{if !is_self(&s){return Ok(s);}}Err(windows::core::Error::from(windows::core::HRESULT(0x80004005u32 as i32)))}
 fn read(s:&Session)->windows::core::Result<State>{let p=s.TryGetMediaPropertiesAsync()?.get()?;let t=s.GetTimelineProperties()?;let playing=s.GetPlaybackInfo()?.PlaybackStatus()?==Status::Playing;let length=t.EndTime()?.Duration as f64/10_000_000.0;let mut position=t.Position()?.Duration as f64/10_000_000.0;if playing{let updated=t.LastUpdatedTime()?.UniversalTime as f64/10_000_000.0;let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs_f64()).unwrap_or(0.0)+11_644_473_600.0;let elapsed=now-updated;if elapsed>0.0&&elapsed<21_600.0{position+=elapsed;}}if length>0.0{position=position.min(length);}Ok(State{active:true,title:p.Title()?.to_string(),artist:p.Artist()?.to_string(),playing,position,length})}
 fn artwork(s:&Session)->Option<Image>{let p=s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;let stream=p.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;let size=stream.Size().ok()? as u32;if size==0||size as u64>3_000_000{return None;}let input=stream.GetInputStreamAt(0).ok()?;let reader=DataReader::CreateDataReader(&input).ok()?;reader.LoadAsync(size).ok()?.get().ok()?;let mut bytes=vec![0u8;size as usize];reader.ReadBytes(&mut bytes).ok()?;let decoded=image::load_from_memory(&bytes).ok()?.thumbnail(256,256).to_rgba8();let(w,h)=decoded.dimensions();Some(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(decoded.as_raw(),w,h)))}
 pub fn poll(weak:Weak<NotchWindow>){std::thread::spawn(move||{
 init();let mut last=String::new();let mut lines:Vec<Line>=Vec::new();let mut lyrics_synced=false;
 loop{
  let mut state=State::default();let mut art:Option<Option<Image>>=None;
  if let Ok(s)=session(){if let Ok(v)=read(&s){
   let key=format!("{}|{}",v.title,v.artist);
   if key!=last{
    last=key.clone();art=Some(artwork(&s));lines.clear();lyrics_synced=false;
    if !v.title.trim().is_empty(){if let Ok(Some(found))=lyrics::fetch(&v.title,&v.artist,v.length){lines=found.lines;lyrics_synced=found.synced;}}
   }
   state=v;
  }}else if !last.is_empty(){last.clear();lines.clear();art=Some(None);}
  let art_update=art.clone();let snapshot=state.clone();
  let lyric_lines:Vec<slint::SharedString>=lines.iter().map(|l|l.text.as_str().into()).collect();
  let lyric_active=active_lyric(&lines,state.position,state.length,lyrics_synced);
  let _=weak.upgrade_in_event_loop(move|w|{
   w.set_media_active(snapshot.active);
   w.set_media_title(if snapshot.active{snapshot.title.clone().into()}else{"Tidak ada media".into()});
   w.set_media_artist(if snapshot.active{snapshot.artist.clone().into()}else{"Putar musik untuk memulai".into()});
   w.set_media_playing(snapshot.playing);w.set_media_position(snapshot.position as f32);w.set_media_length(snapshot.length as f32);
   w.set_media_progress(if snapshot.length>0.0{(snapshot.position/snapshot.length)as f32}else{0.0});
   w.set_media_pos_text(time_text(snapshot.position).into());w.set_media_dur_text(time_text(snapshot.length).into());
   w.set_lyric_lines(slint::ModelRc::from(std::rc::Rc::new(slint::VecModel::from(lyric_lines))));
   w.set_lyric_active(lyric_active);
   if let Some(a)=art_update{if let Some(img)=a{w.set_media_art(img);w.set_media_has_art(true);}else{w.set_media_has_art(false);}}
  });
  std::thread::sleep(Duration::from_millis(if state.active{1000}else{2000}));
 }
});}
 pub fn control(action:&str){let a=action.to_string();std::thread::spawn(move||{if let Ok(s)=session(){let _=match a.as_str(){"toggle"=>s.TryTogglePlayPauseAsync().map(|_|()),"pause"=>s.TryPauseAsync().map(|_|()),"play"=>s.TryPlayAsync().map(|_|()),"next"=>s.TrySkipNextAsync().map(|_|()),"prev"=>s.TrySkipPreviousAsync().map(|_|()),_=>Ok(())};}});}
 pub fn seek(seconds:f64){std::thread::spawn(move||{if let Ok(s)=session(){let ticks=(seconds.max(0.0)*10_000_000.0)as i64;let _=s.TryChangePlaybackPositionAsync(ticks);}});}
}
#[cfg(not(windows))]
mod platform{pub fn poll(_:slint::Weak<crate::NotchWindow>){}pub fn control(_: &str){}pub fn seek(_:f64){}}
pub fn start(w:Weak<NotchWindow>){platform::poll(w);}
pub fn control(a:&str){platform::control(a);}
pub fn seek(s:f64){platform::seek(s);}
