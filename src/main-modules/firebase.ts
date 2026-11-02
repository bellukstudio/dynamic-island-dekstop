import { getApp, getApps, initializeApp, type FirebaseApp } from "firebase/app";
import { doc, getDoc, getFirestore, type Firestore } from "firebase/firestore";

/** Data update yang dibaca dari Firestore: notch_config/update_config. */
export type RemoteUpdateConfig = {
  latest_version: string;
  link_update: string;
  new_version: string;
};

let firebaseApp: FirebaseApp | undefined;
let firestore: Firestore | undefined;

function getFirebaseDb(): Firestore {
  const projectId = import.meta.env.VITE_FIREBASE_PROJECT_ID as string | undefined;
  const apiKey = import.meta.env.VITE_FIREBASE_API_KEY as string | undefined;

  if (!projectId || !apiKey) {
    console.info("[Firebase env check]", {
      mode: import.meta.env.MODE,
      projectIdPresent: Boolean(projectId),
      apiKeyPresent: Boolean(apiKey),
      vitePrefixedKeys: Object.keys(import.meta.env).filter((k) => k.startsWith("VITE_")),
    });
    throw new Error(
      "Konfigurasi Firebase belum lengkap. Isi VITE_FIREBASE_PROJECT_ID dan VITE_FIREBASE_API_KEY di file .env di root proyek, lalu restart Vite."
    );
  }

  firebaseApp ??= getApps().length ? getApp() : initializeApp({ apiKey, projectId });
  firestore ??= getFirestore(firebaseApp);
  return firestore;
}

/**
 * Mengambil dokumen Firestore:
 * collections: notch_config
 * document:    update_config
 *
 * Dokumen tersebut hanya dibaca. Fungsi ini tidak menulis ke Firestore.
 */
export async function loadRemoteUpdateConfig(): Promise<RemoteUpdateConfig> {
  const db = getFirebaseDb();
  const reference = doc(db, "notch_config", "update_config");
  const snapshot = await getDoc(reference);

  if (!snapshot.exists()) {
    throw new Error(
      'Dokumen Firestore "notch_config/update_config" tidak ditemukan. Periksa nama collection dan document ID.'
    );
  }

  const data = snapshot.data();
  const latestVersion = data.latest_version;
  const updateLink = data.link_update;
  const newVersion = data.new_version;

  if (
    typeof latestVersion !== "string" ||
    typeof updateLink !== "string" ||
    typeof newVersion !== "string"
  ) {
    throw new Error(
      'Field latest_version, link_update, dan new_version harus bertipe String di Firestore.'
    );
  }

  return {
    latest_version: latestVersion,
    link_update: updateLink,
    new_version: newVersion,
  };
}
