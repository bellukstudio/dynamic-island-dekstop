//! Native Windows media session integration.
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Weak};
use crate::{NotchWindow, Theme};
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
 use std::sync::Mutex;
 static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());
 fn init(){unsafe{let _=RoInitialize(RO_INIT_MULTITHREADED);}}
 fn app_id(s:&Session)->String{s.SourceAppUserModelId().map(|x|x.to_string()).unwrap_or_default().to_lowercase()}
 fn is_self(s:&Session)->bool{let id=app_id(s);if id.contains("webview2"){return true;}std::env::current_exe().ok().and_then(|p|p.file_stem().map(|x|x.to_string_lossy().to_lowercase())).map(|n|n.len()>=3&&id.contains(&n)).unwrap_or(false)}
 fn session()->windows::core::Result<Session>{init();let m=Manager::RequestAsync()?.get()?;if let Ok(s)=m.GetCurrentSession(){if !is_self(&s){return Ok(s);}}for s in m.GetSessions()?{if !is_self(&s){return Ok(s);}}Err(windows::core::Error::from(windows::core::HRESULT(0x80004005u32 as i32)))}
 fn read(s:&Session)->windows::core::Result<State>{let p=s.TryGetMediaPropertiesAsync()?.get()?;let t=s.GetTimelineProperties()?;let playing=s.GetPlaybackInfo()?.PlaybackStatus()?==Status::Playing;let length=t.EndTime()?.Duration as f64/10_000_000.0;let mut position=t.Position()?.Duration as f64/10_000_000.0;if playing{let updated=t.LastUpdatedTime()?.UniversalTime as f64/10_000_000.0;let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs_f64()).unwrap_or(0.0)+11_644_473_600.0;let elapsed=now-updated;if elapsed>0.0&&elapsed<21_600.0{position+=elapsed;}}if length>0.0{position=position.min(length);}Ok(State{active:true,title:p.Title()?.to_string(),artist:p.Artist()?.to_string(),playing,position,length})}
 /// Renderer software Slint tidak memotong gambar mengikuti `border-radius`, jadi sudut cover dibulatkan
 /// langsung di piksel: potong persegi di tengah lalu kurangi alpha di sudut (radius 20% sisi, setara 16 px pada 80 px di Tauri).
 /// Pilih warna aksen dari warna dominan artwork. Warna netral/terlalu gelap/terang
 /// diberi bobot lebih rendah agar aksen tetap terlihat jelas di atas UI.
 fn dominant_color(img:&image::RgbaImage)->[u8;3]{
  use std::collections::HashMap;
  let mut buckets:HashMap<u16,(u32,u64,u64,u64,f32)>=HashMap::new();
  for(_,_,p)in img.enumerate_pixels(){
   let [r,g,b,a]=p.0;
   if a<180{continue;}
   let rf=r as f32/255.0;let gf=g as f32/255.0;let bf=b as f32/255.0;
   let max=rf.max(gf).max(bf);let min=rf.min(gf).min(bf);
   let saturation=if max<=0.0{0.0}else{(max-min)/max};
   let brightness=(0.2126*rf+0.7152*gf+0.0722*bf);
   if brightness<0.12||brightness>0.96||saturation<0.12{continue;}
   let key=(((r>>3)as u16)<<10)|(((g>>3)as u16)<<5)|((b>>3)as u16);
   let e=buckets.entry(key).or_insert((0,0,0,0,0.0));
   e.0+=1;e.1+=r as u64;e.2+=g as u64;e.3+=b as u64;
   e.4+=saturation*(0.55+0.45*(1.0-(brightness-0.55).abs()));
  }
  if let Some((_,(count,sr,sg,sb,_)))=buckets.iter().max_by(|a,b|{
   let score=|v:&(u32,u64,u64,u64,f32)|v.4*(v.0 as f32).sqrt();
   score(a.1).total_cmp(&score(b.1))
  }){
   let n=*count as u64;
   if n>0{return [(*sr/n)as u8,(*sg/n)as u8,(*sb/n)as u8];}
  }
  // Fallback: warna rata-rata seluruh piksel yang tidak transparan.
  let(mut n,mut r,mut g,mut b)=(0u64,0u64,0u64,0u64);
  for(_,_,p)in img.enumerate_pixels(){if p.0[3]>=180{n+=1;r+=p.0[0]as u64;g+=p.0[1]as u64;b+=p.0[2]as u64;}}
  if n==0{[120,140,255]}else{[(r/n)as u8,(g/n)as u8,(b/n)as u8]}
 }
 fn round_cover(img:image::RgbaImage)->(Vec<u8>,u32,u32){
  let(w,h)=img.dimensions();let side=w.min(h);
  let mut out=image::imageops::crop_imm(&img,(w-side)/2,(h-side)/2,side,side).to_image();
  let s=side as f32;let r=s*0.2;
  for(x,y,px)in out.enumerate_pixels_mut(){
   let fx=x as f32+0.5;let fy=y as f32+0.5;
   let cx=fx.clamp(r,s-r);let cy=fy.clamp(r,s-r);
   let d=((fx-cx)*(fx-cx)+(fy-cy)*(fy-cy)).sqrt();
   let cover=(r-d+0.5).clamp(0.0,1.0);
   if cover<1.0{px.0[3]=(px.0[3] as f32*cover).round() as u8;}
  }
  let(ow,oh)=out.dimensions();(out.into_raw(),ow,oh)
 }
 fn artwork(s:&Session)->Option<(Vec<u8>,u32,u32,[u8;3])>{let p=s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;let stream=p.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;let size=stream.Size().ok()? as u32;if size==0||size as u64>3_000_000{return None;}let input=stream.GetInputStreamAt(0).ok()?;let reader=DataReader::CreateDataReader(&input).ok()?;reader.LoadAsync(size).ok()?.get().ok()?;let mut bytes=vec![0u8;size as usize];reader.ReadBytes(&mut bytes).ok()?;let decoded=image::load_from_memory(&bytes).ok()?.thumbnail(256,256).to_rgba8();let accent=dominant_color(&decoded);let(rgba,width,height)=round_cover(decoded);Some((rgba,width,height,accent))}
 pub fn poll(weak:Weak<NotchWindow>,auto_accent:bool,fallback_accent:slint::Color){std::thread::spawn(move||{
 init();let mut last=String::new();let mut lines:Vec<Line>=Vec::new();let mut lyrics_synced=false;
 loop{
  let mut state=State::default();let mut art:Option<Option<(Vec<u8>,u32,u32)>>=None;
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
   if let Some(a)=art_update{if let Some((rgba,width,height,rgb))=a{let img=Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba,width,height));w.set_media_art(img);w.set_media_has_art(true);if auto_accent{let accent=slint::Color::from_rgb_u8(rgb[0],rgb[1],rgb[2]);Theme::get(&w).set_accent(accent);let luma=0.2126*rgb[0]as f32+0.7152*rgb[1]as f32+0.0722*rgb[2]as f32;Theme::get(&w).set_on_accent(if luma>145.0{slint::Color::from_rgb_u8(0,0,0)}else{slint::Color::from_rgb_u8(255,255,255)});}}else{w.set_media_has_art(false);if auto_accent{Theme::get(&w).set_accent(fallback_accent);Theme::get(&w).set_on_accent(slint::Color::from_rgb_u8(255,255,255));}}}
  });
  std::thread::sleep(Duration::from_millis(if state.active{1000}else{2000}));
 }
});}
 pub fn control(action:&str){let a=action.to_string();std::thread::spawn(move||{if let Ok(s)=session(){let _=match a.as_str(){"toggle"=>s.TryTogglePlayPauseAsync().map(|_|()),"pause"=>s.TryPauseAsync().map(|_|()),"play"=>s.TryPlayAsync().map(|_|()),"next"=>s.TrySkipNextAsync().map(|_|()),"prev"=>s.TrySkipPreviousAsync().map(|_|()),_=>Ok(())};}});}
 pub fn seek(seconds:f64){std::thread::spawn(move||{if let Ok(s)=session(){let ticks=(seconds.max(0.0)*10_000_000.0)as i64;let _=s.TryChangePlaybackPositionAsync(ticks);}});}
 /// Jeda semua sesi media pengguna dan simpan hanya sesi yang benar-benar sedang diputar.
 pub fn pause_all()->usize{
  init();
  let mut paused=Vec::new();
  if let Ok(manager)=Manager::RequestAsync().and_then(|op|op.get()){
   if let Ok(sessions)=manager.GetSessions(){
    for s in sessions{
     if is_self(&s){continue;}
     let playing=s.GetPlaybackInfo().and_then(|i|i.PlaybackStatus()).map(|v|v==Status::Playing).unwrap_or(false);
     if !playing{continue;}
     if s.TryPauseAsync().and_then(|op|op.get()).is_ok(){paused.push(app_id(&s));}
    }
   }
  }
  let count=paused.len();
  *PAUSED.lock().unwrap_or_else(|e|e.into_inner())=paused;
  count
 }
 /// Lanjutkan hanya sesi yang sebelumnya dijeda oleh pause_all.
 pub fn resume_paused(){
  init();
  let ids=std::mem::take(&mut *PAUSED.lock().unwrap_or_else(|e|e.into_inner()));
  if ids.is_empty(){return;}
  if let Ok(manager)=Manager::RequestAsync().and_then(|op|op.get()){
   if let Ok(sessions)=manager.GetSessions(){
    for s in sessions{
     if !is_self(&s)&&ids.contains(&app_id(&s)){let _=s.TryPlayAsync().and_then(|op|op.get());}
    }
   }
  }
 }

 /// Kontrol pemutar adzan native. Tidak bergantung pada driver Windows MCI/mpegvideo.
 use rodio::{Decoder, OutputStream, Sink};
 use std::fs::File;
 use std::io::BufReader;
 use std::sync::Arc;
 use std::sync::atomic::{AtomicBool, Ordering};

 static ADHAN_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
 static ACTIVE_ADHAN: Mutex<Option<Arc<Sink>>> = Mutex::new(None);

 pub fn stop_adhan(){
  ADHAN_STOP_REQUESTED.store(true, Ordering::SeqCst);
  if let Some(sink)=ACTIVE_ADHAN.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){
   sink.stop();
  }
 }
 /// Jeda atau lanjutkan pemutaran adzan yang sedang aktif.
 pub fn pause_adhan(pause:bool){
  if let Some(sink)=ACTIVE_ADHAN.lock().unwrap_or_else(|e|e.into_inner()).as_ref(){
   if pause { sink.pause(); } else { sink.play(); }
  }
 }
 pub fn play_adhan(weak:Weak<NotchWindow>,prayer_name:String){
  ADHAN_STOP_REQUESTED.store(false, Ordering::SeqCst);
  let _=weak.upgrade_in_event_loop({let prayer_name=prayer_name.clone();move|w|{
   w.set_adhan_prayer(prayer_name.into());
   w.set_adhan_active(true);
   w.set_prayer_status("Menyiapkan pemutar adzan...".into());
  }});
  std::thread::spawn(move||{
   let paused=pause_all();
   let result=(||->Result<(),String>{
    let path=notch_core::paths::data_file("adzan.mp3");
    if let Some(parent)=path.parent(){
     std::fs::create_dir_all(parent).map_err(|e|format!("Gagal membuat folder audio: {e}"))?;
    }
    if !path.exists(){
     std::fs::write(&path,include_bytes!("../assets/adzan.mp3"))
      .map_err(|e|format!("Gagal menyalin audio adzan: {e}"))?;
    }
    if ADHAN_STOP_REQUESTED.load(Ordering::SeqCst){return Ok(());}
    let file=File::open(&path).map_err(|e|format!("Tidak dapat membuka {}: {e}",path.display()))?;
    let decoder=Decoder::new(BufReader::new(file))
     .map_err(|e|format!("Format audio adzan tidak dapat dibaca: {e}"))?;
    // Stream harus tetap hidup selama Sink memutar audio.
    let (_stream,handle)=OutputStream::try_default()
     .map_err(|e|format!("Perangkat audio Windows tidak tersedia: {e}"))?;
    let sink=Sink::try_new(&handle)
     .map_err(|e|format!("Gagal membuat pemutar audio: {e}"))?;
    sink.set_volume(1.0);
    sink.append(decoder);
    let sink=Arc::new(sink);
    *ACTIVE_ADHAN.lock().unwrap_or_else(|e|e.into_inner())=Some(sink.clone());
    if ADHAN_STOP_REQUESTED.load(Ordering::SeqCst){sink.stop();}
    sink.sleep_until_end();
    let mut active=ACTIVE_ADHAN.lock().unwrap_or_else(|e|e.into_inner());
    if active.as_ref().is_some_and(|current|Arc::ptr_eq(current,&sink)){*active=None;}
    Ok(())
   })();
   // Pastikan state pemutar dibersihkan meskipun perangkat audio atau decoder gagal.
   ACTIVE_ADHAN.lock().unwrap_or_else(|e|e.into_inner()).take();
   resume_paused();
   let stopped=ADHAN_STOP_REQUESTED.swap(false,Ordering::SeqCst);
   let message=if stopped{
    format!("Adzan {prayer_name} dihentikan")
   }else{
    match result{
     Ok(())=>format!("Adzan {prayer_name} selesai"),
     Err(e)=>format!("Adzan gagal diputar: {e}"),
    }
   };
   let _=weak.upgrade_in_event_loop(move|w|{
    w.set_adhan_active(false);
    w.set_prayer_status(message.into());
    w.invoke_adhan_finished();
   });
   let _=paused;
  });
 }
}

#[cfg(not(windows))]
mod platform{pub fn poll(_:slint::Weak<crate::NotchWindow>,_:bool,_:slint::Color){}pub fn control(_: &str){}pub fn seek(_:f64){}pub fn pause_all()->usize{0}pub fn resume_paused(){}pub fn play_adhan(_:slint::Weak<crate::NotchWindow>,_:String){} pub fn stop_adhan(){} pub fn pause_adhan(_:bool){}}
pub fn start(w:Weak<NotchWindow>,auto_accent:bool,fallback_accent:slint::Color){platform::poll(w,auto_accent,fallback_accent);}
pub fn control(a:&str){platform::control(a);}
pub fn seek(s:f64){platform::seek(s);}
/// Jeda sesi media pengguna sebelum audio prioritas (mis. adzan) diputar.
pub fn pause_all()->usize{platform::pause_all()}
/// Lanjutkan kembali sesi yang dijeda oleh pause_all.
pub fn resume_paused(){platform::resume_paused();}
/// Putar audio adzan bawaan sambil menjeda lalu memulihkan media pengguna.
pub fn play_adhan(w:Weak<NotchWindow>,name:String){platform::play_adhan(w,name);}
/// Hentikan audio adzan yang sedang berjalan.
pub fn stop_adhan(){platform::stop_adhan();}
pub fn pause_adhan(pause:bool){platform::pause_adhan(pause);}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn synced_lyrics_highlight_latest_timestamp() {
  let lines=vec![Line{at:2.0,text:"one".into()},Line{at:5.0,text:"two".into()},Line{at:9.0,text:"three".into()}];
  assert_eq!(active_lyric(&lines,1.0,20.0,true),-1);
  assert_eq!(active_lyric(&lines,5.5,20.0,true),1);
  assert_eq!(active_lyric(&lines,19.0,20.0,true),2);
 }
 #[test]
 fn plain_lyrics_use_playback_fraction_without_claiming_sync() {
  let lines=vec![Line{at:0.0,text:"one".into()},Line{at:0.0,text:"two".into()},Line{at:0.0,text:"three".into()},Line{at:0.0,text:"four".into()}];
  assert_eq!(active_lyric(&lines,0.0,100.0,false),0);
  assert_eq!(active_lyric(&lines,51.0,100.0,false),2);
  assert_eq!(active_lyric(&lines,99.0,100.0,false),3);
  assert_eq!(active_lyric(&lines,1.0,0.0,false),-1);
 }
 #[test]
 fn empty_lyrics_have_no_active_line() {
  assert_eq!(active_lyric(&[],10.0,100.0,true),-1);
 }
}
