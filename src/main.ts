import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { initNudge } from "./main-modules/nudge";
import { initGreeting } from "./main-modules/greeting";
import { openExternalUrl } from "./main-modules/open-external";
import { initMoments, setMeetingMode, noteMedia, noteWeather, noteBattery, noteActivity, focusStarted, focusTick, focusFinished } from "./main-modules/moments";
import { loadFace, saveFace, startClock } from "./main-modules/clock";
import { initCalendar, goToday, shiftMonth } from "./main-modules/calendar";
import { initHubNav } from "./main-modules/hub-nav";
import { initPrayer } from "./main-modules/prayer";
import { initBottomPlay } from "./main-modules/bottom-play";
import { getVersion } from "@tauri-apps/api/app";
import { loadRemoteUpdateConfig, type RemoteUpdateConfig } from "./main-modules/firebase";
import { initEyeBreak } from "./main-modules/eye-break";
import { initPriceAlert } from "./main-modules/price-alert";
import { initMoodMouth } from "./main-modules/mood-mouth";
import { initAdhanFace } from "./main-modules/adhan-face";
import { initMomentBubble } from "./main-modules/moment-bubble";
import { initWhatsappNotifications } from "./main-modules/whatsapp-notifications";
declare global {
    interface Window {
        __NOTCH_UPDATE_CONFIG__?: RemoteUpdateConfig;
    }
}

import * as configState from './main-modules/config-state';
import * as windowLayout from './main-modules/window-layout';
import * as viewController from './main-modules/view-controller';
import * as mood from './main-modules/mood';
import * as hubNavigation from './main-modules/hub-navigation';
import * as hover from './main-modules/hover';
import * as windowDragOrb from './main-modules/window-drag-orb';
import * as notchFace from './main-modules/notch-face';
import * as media from './main-modules/media';
import * as devices from './main-modules/devices';
import * as weatherHud from './main-modules/weather-hud';
import * as pomodoro from './main-modules/pomodoro';
import * as quickStats from './main-modules/quick-stats';
import * as clipboardPrivacy from './main-modules/clipboard-privacy';
import * as colorPicker from './main-modules/color-picker';
import * as lyrics from './main-modules/lyrics';
import * as shelf from './main-modules/shelf';
import * as reminders from './main-modules/reminders';
import * as notes from './main-modules/notes';
import * as keyboardDemo from './main-modules/keyboard-demo';
import * as integrations from './main-modules/integrations';
import * as updates from './main-modules/updates';
import * as bottomPlayStartup from './main-modules/bottom-play-startup';

// Shared runtime state: implementations live in focused modules under main-modules/.
const ctx: Record<string, any> = {
  focusFinished,
  focusStarted,
  focusTick,
  getCurrentWebview,
  getVersion,
  goToday,
  initBottomPlay,
  initCalendar,
  initEyeBreak,
  initGreeting,
  initHubNav,
  initMoments,
  initNudge,
  initPrayer,
  initPriceAlert,
  invoke,
  listen,
  loadFace,
  loadRemoteUpdateConfig,
  noteActivity,
  noteBattery,
  noteMedia,
  noteWeather,
  openExternalUrl,
  saveFace,
  shiftMonth,
  startClock,
  setMomentsMeeting: setMeetingMode
};

// Register function declarations first to preserve their original hoisting behavior.
  configState.register(ctx);
  
  windowLayout.register(ctx);
  viewController.register(ctx);
  mood.register(ctx);
  hubNavigation.register(ctx);
  hover.register(ctx);
  windowDragOrb.register(ctx);
  notchFace.register(ctx);
  media.register(ctx);
  devices.register(ctx);
  weatherHud.register(ctx);
  pomodoro.register(ctx);
  quickStats.register(ctx);
  clipboardPrivacy.register(ctx);
  colorPicker.register(ctx);
  lyrics.register(ctx);
  shelf.register(ctx);
  reminders.register(ctx);
  notes.register(ctx);
  keyboardDemo.register(ctx);
  integrations.register(ctx);
  updates.register(ctx);
  bottomPlayStartup.register(ctx);

// Execute module initialization in the same order as the original main.ts.
  configState.init(ctx);
  ctx.initCalendar(); 
  // Restore the Conversation Moments / Focus Companion speech cloud.
  ctx.momentBubble = initMomentBubble({
    shell: ctx.$("shell"),
    notch: ctx.notch,
    // awan bertombol (pengingat) harus ikut masuk area klik native, kalau tidak klik tembus ke aplikasi lain
    onLayout: () => ctx.setHit?.(ctx.notch.dataset.view ?? "idle"),
  });
  // Restore idle mood-mouth and adhan mouth/face animations.
  initMoodMouth(ctx.eyes);
  ctx.adhanFace = initAdhanFace(ctx.notch, ctx.eyes);
  windowLayout.init(ctx);
  viewController.init(ctx);
  mood.init(ctx);
  hubNavigation.init(ctx);
  hover.init(ctx);
  windowDragOrb.init(ctx);
  notchFace.init(ctx);
  media.init(ctx);
  devices.init(ctx);
  weatherHud.init(ctx);
  pomodoro.init(ctx);
  quickStats.init(ctx);
  clipboardPrivacy.init(ctx);
  colorPicker.init(ctx);
  lyrics.init(ctx);
  shelf.init(ctx);
  reminders.init(ctx);
  notes.init(ctx);
  keyboardDemo.init(ctx);
  integrations.init(ctx);
  updates.init(ctx);
  bottomPlayStartup.init(ctx);
  void initWhatsappNotifications({
    canShow: () => !ctx.meetingMode && !ctx.prayerAdhanActive && ctx.override === null && !ctx.isDraggingWindow,
    show: (title, sub, ms) => {
      // Dock kiri/kanan/bawah: awan percakapan; di atas: kartu teks bergaya notifikasi lain.
      if (ctx.momentBubble?.show(`${title} 💬`, sub, ms, 360) === true) return;
      ctx.showTextCard?.(title, sub, ms, "💬");
      // Satu baris + elipsis. Dilepas oleh showTextCard berikutnya, atau otomatis setelah kartu hilang
      // (jalur lain seperti devices.ts mengisi #dev-sub langsung tanpa showTextCard).
      const subEl = ctx.$("dev-sub");
      subEl.classList.add("wa-oneline");
      window.setTimeout(() => subEl.classList.remove("wa-oneline"), ms + 800);
    },
    mood: (m, ms) => ctx.flash(m, ms),
  }).catch((error) =>
    console.error("[WhatsApp] Gagal mengaktifkan notifikasi notch:", error)
  );

// Preserve the original public helpers.
export const setBase = (...args: any[]) => ctx.setBase(...args);
export const flash = (...args: any[]) => ctx.flash(...args);