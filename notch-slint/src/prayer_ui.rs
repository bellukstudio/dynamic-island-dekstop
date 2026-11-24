//! Halaman Adzan/Ramadan: jadwal salat, hitung mundur, dan render ke UI.

use crate::*;

#[derive(Clone, Default)]
pub(crate) struct PrayerSchedule { pub(crate) current: Vec<PrayerDay>, pub(crate) next: Vec<PrayerDay>, pub(crate) error: String }
pub(crate) fn prayer_at(date:(i64,i64,i64),time:&str,offset:i32)->Option<i64>{let(y,m,d)=date;reminder::parse_local_datetime(&format!("{d:02}/{m:02}/{y} {time}"),offset)}
pub(crate) fn prayer_countdown(target:Option<i64>,now:i64)->String{let Some(t)=target else{return "--:--:--".into()};let s=(t-now).max(0)/1000;format!("{:02}:{:02}:{:02}",s/3600,(s%3600)/60,s%60)}
pub(crate) fn prayer_find(days:&[PrayerDay],day:i64)->Option<PrayerDay>{days.iter().find(|x|i64::from(x.tanggal)==day).cloned()}
pub(crate) fn render_prayer(w:&NotchWindow,s:&PrayerSchedule,offset:i32,ramadan:bool){
 let now=reminder::now_ms();let(y,m,d,_,_)=reminder::local_parts(now,offset);let(ny,nm,nd,_,_)=reminder::local_parts(now+86_400_000,offset);
 let today=prayer_find(&s.current,d);let tomorrow=if y==ny&&m==nm{today.clone()}else{prayer_find(&s.next,nd)};
 let entries:[(&str,&str);5]=today.as_ref().map(|x|[("Subuh",x.subuh.as_str()),("Dzuhur",x.dzuhur.as_str()),("Ashar",x.ashar.as_str()),("Maghrib",x.maghrib.as_str()),("Isya",x.isya.as_str())]).unwrap_or([("Subuh",""),("Dzuhur",""),("Ashar",""),("Maghrib",""),("Isya","")]);
 let mut next:Option<(&str,&str,i64)>=None;for(name,time)in entries{if let Some(at)=prayer_at((y,m,d),time,offset){if at>now{next=Some((name,time,at));break;}}}
 if next.is_none(){if let Some(t)=tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.subuh,offset)){next=Some(("Subuh",tomorrow.as_ref().unwrap().subuh.as_str(),t));}}
 if let Some((name,time,at))=next{w.set_prayer_next_name(name.into());w.set_prayer_next_time(time.into());w.set_prayer_countdown(prayer_countdown(Some(at),now).into());}else{w.set_prayer_next_name("Jadwal belum tersedia".into());w.set_prayer_next_time("--:--".into());w.set_prayer_countdown("--:--:--".into());}
 let rows=entries.into_iter().map(|(name,time)|{let at=prayer_at((y,m,d),time,offset);let state=if next.map(|x|x.0)==Some(name){1}else if at.map(|t|t<=now).unwrap_or(false){2}else{0};PrayerItem{name:name.into(),time:if time.is_empty(){"--:--".into()}else{time.into()},state}}).collect::<Vec<_>>();
 w.set_prayer_items(ModelRc::from(Rc::new(VecModel::from(rows))));w.set_prayer_status(s.error.clone().into());
 {let tm=|t:&str|if t.is_empty(){"--:--".to_string()}else{t.to_string()};
  let (im,te)=today.as_ref().map(|x|(tm(&x.imsak),tm(&x.terbit))).unwrap_or(("--:--".into(),"--:--".into()));
  w.set_prayer_meta(format!("Imsak {im} \u{00B7} Terbit {te}").into());
  let tomorrow_next=next.map(|x|reminder::local_parts(x.2,offset).2!=d).unwrap_or(false);
  w.set_prayer_when(if next.is_none(){""}else if tomorrow_next{"Besok"}else{"Hari ini"}.into());
  let sheet:Vec<(&str,String)>=match today.as_ref(){Some(x)=>vec![("Imsak",tm(&x.imsak)),("Subuh",tm(&x.subuh)),("Terbit",tm(&x.terbit)),("Dzuhur",tm(&x.dzuhur)),("Ashar",tm(&x.ashar)),("Maghrib",tm(&x.maghrib)),("Isya",tm(&x.isya))],None=>["Imsak","Subuh","Terbit","Dzuhur","Ashar","Maghrib","Isya"].into_iter().map(|n|(n,"--:--".to_string())).collect()};
  w.set_prayer_imsakiyah(ModelRc::from(Rc::new(VecModel::from(sheet.into_iter().map(|(n,t)|PrayerItem{name:n.into(),time:t.into(),state:0}).collect::<Vec<_>>()))));}
 if ramadan{let im=today.as_ref().and_then(|x|prayer_at((y,m,d),&x.imsak,offset)).filter(|t|*t>now).or_else(||tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.imsak,offset)));let bu=today.as_ref().and_then(|x|prayer_at((y,m,d),&x.maghrib,offset)).filter(|t|*t>now).or_else(||tomorrow.as_ref().and_then(|x|prayer_at((ny,nm,nd),&x.maghrib,offset)));{let (_,wd)=platform::local_sec_wday();w.set_ramadan_date(format!("{}, {d} {} {y}",ID_DAYS[(wd%7) as usize],ID_MONTHS[(m.clamp(1,12)-1) as usize]).into());}w.set_ramadan_imsak_countdown(prayer_countdown(im,now).into());w.set_ramadan_buka_countdown(prayer_countdown(bu,now).into());w.set_ramadan_imsak_time(format!("Imsak {}",today.as_ref().map(|x|x.imsak.as_str()).unwrap_or("--:--")).into());w.set_ramadan_buka_time(format!("Maghrib {}",today.as_ref().map(|x|x.maghrib.as_str()).unwrap_or("--:--")).into());}
}
pub(crate) fn load_ramadan()->bool{std::fs::read_to_string(notch_core::paths::data_file("ramadan.json")).ok().map(|s|s.trim()=="true").unwrap_or(false)}
