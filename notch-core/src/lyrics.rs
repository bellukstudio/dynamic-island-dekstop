//! LRCLIB lyrics lookup and LRC parsing shared by native frontends.
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Line { pub at: f64, pub text: String }
#[derive(Clone, Debug, Default)]
pub struct Lyrics { pub synced: bool, pub lines: Vec<Line> }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Record {
    #[serde(default)] synced_lyrics: Option<String>,
    #[serde(default)] plain_lyrics: Option<String>,
    #[serde(default)] instrumental: bool,
    #[serde(default)] duration: Option<f64>,
}
const API:&str="https://lrclib.net/api";
fn enc(s:&str)->String {
 let mut out=String::new();
 for b in s.bytes() { if b.is_ascii_alphanumeric()||b"-_.~".contains(&b) {out.push(b as char);} else {out.push_str(&format!("%{b:02X}"));} }
 out
}
fn get(path:&str,params:&[(&str,String)])->Result<String,String>{
 let query=params.iter().map(|(k,v)|format!("{}={}",enc(k),enc(v))).collect::<Vec<_>>().join("&");
 let url=if query.is_empty(){format!("{API}/{path}")}else{format!("{API}/{path}?{query}")};
 crate::http::get_text(&url,8)
}
pub fn fetch(title:&str,artist:&str,length:f64)->Result<Option<Lyrics>,String>{
 let title=clean_title(title);let artist=artist.trim().strip_suffix(" - Topic").unwrap_or(artist.trim()).trim().to_string();
 if title.is_empty(){return Ok(None);}
 let mut params=vec![("track_name",title.clone()),("artist_name",artist.clone())];
 if length>0.0{params.push(("duration",(length.round()as i64).to_string()));}
 if let Ok(exact)=get("get",&params){if let Ok(rec)=serde_json::from_str::<Record>(&exact){if let Some(l)=convert(&rec){return Ok(Some(l));}}}
 let search=get("search",&[("track_name",title),("artist_name",artist)])?;
 let records:Vec<Record>=serde_json::from_str(&search).map_err(|e|e.to_string())?;
 let mut best:Option<(f64,Lyrics)>=None;
 for rec in &records {
  let Some(l)=convert(rec) else{continue};
  let diff=match(length>0.0,rec.duration){(true,Some(d))=>(d-length).abs(),_=>0.0};
  if length>0.0&&rec.duration.is_some()&&diff>6.0{continue;}
  let score=diff+if l.synced{0.0}else{100.0};
  if best.as_ref().map_or(true,|(s,_)|score<*s){best=Some((score,l));}
 }
 Ok(best.map(|(_,l)|l))
}
fn convert(r:&Record)->Option<Lyrics>{
 if r.instrumental{return None;}
 if let Some(s)=r.synced_lyrics.as_deref(){let lines=parse_lrc(s);if !lines.is_empty(){return Some(Lyrics{synced:true,lines});}}
 if let Some(s)=r.plain_lyrics.as_deref(){let lines=s.lines().map(|x|Line{at:0.0,text:x.trim().to_string()}).collect::<Vec<_>>();if lines.iter().any(|x|!x.text.is_empty()){return Some(Lyrics{synced:false,lines});}}
 None
}
pub fn parse_lrc(src:&str)->Vec<Line>{
 let mut out=Vec::new();
 for raw in src.lines(){let mut rest=raw.trim();let mut times=Vec::new();while rest.starts_with('['){let Some(end)=rest.find(']')else{break};if let Some(t)=timestamp(&rest[1..end]){times.push(t);rest=rest[end+1..].trim_start();}else{break;}}
 if times.is_empty(){continue;}let text=strip_word_tags(rest).trim().to_string();for at in times{out.push(Line{at,text:text.clone()});}}
 out.sort_by(|a,b|a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));out
}
fn timestamp(s:&str)->Option<f64>{let(m,sec)=s.split_once(':')?;let m:u32=m.trim().parse().ok()?;let sec=sec.trim().replace(',','.');if sec.is_empty()||!sec.chars().all(|c|c.is_ascii_digit()||c=='.'){return None;}Some(m as f64*60.0+sec.parse::<f64>().ok()?)}
fn strip_word_tags(s:&str)->String{
 let mut out=String::with_capacity(s.len());let mut chars=s.chars().peekable();
 while let Some(c)=chars.next(){if c=='<'{let mut tag=String::new();let mut closed=false;for n in chars.by_ref(){if n=='>'{closed=true;break;}tag.push(n);}if closed&&timestamp(&tag).is_some(){continue;}out.push('<');out.push_str(&tag);if closed{out.push('>');}}else{out.push(c);}}
 out
}
fn clean_title(s:&str)->String{
 let mut t=s.trim().to_string();const NOISE:[&str;9]=["official","video","audio","lyric","lyrics","visualizer","hd","4k","remaster"];
 loop{let Some(i)=t.rfind(|c|c=='('||c=='[')else{break};let tail=t[i..].to_lowercase();if (tail.ends_with(')')||tail.ends_with(']'))&&NOISE.iter().any(|n|tail.contains(n)){t.truncate(i);t=t.trim_end().to_string();}else{break;}}
 t.trim().to_string()
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test]fn parses_sorted_multi_timestamp_lines(){let l=parse_lrc("[ar:Foo]\n[00:12.50]Halo\n[01:02.5][00:05.00]Dunia\n[00:20.00]");assert_eq!(l.len(),4);assert_eq!(l[0].text,"Dunia");assert!((l[0].at-5.0).abs()<1e-6);assert!((l[2].at-20.0).abs()<1e-6);assert_eq!(l[2].text,"");}
 #[test]fn strips_enhanced_word_tags(){assert_eq!(strip_word_tags("<00:01.00>Ha<00:01.50>lo"),"Halo");}
 #[test]fn cleans_only_noisy_suffix(){assert_eq!(clean_title("Song (Official Video)"),"Song");assert_eq!(clean_title("Song (feat. X)"),"Song (feat. X)");}
}
