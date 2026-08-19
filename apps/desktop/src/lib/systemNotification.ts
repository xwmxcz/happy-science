import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { PRODUCT_NAME } from "@ai4s/shared";

export interface PermissionNotificationInput {
  action: string;
  resources: string[];
}

function permissionBody(input: PermissionNotificationInput): string {
  const firstResource = input.resources[0];
  return firstResource ? `${input.action}\n${firstResource}` : input.action;
}

export async function notifyPermissionRequest(input: PermissionNotificationInput): Promise<boolean> {
  let granted = await isPermissionGranted();
  if (!granted) {
    granted = (await requestPermission()) === "granted";
  }
  if (!granted) return false;

  try {
    sendNotification({
      title: `${PRODUCT_NAME} needs your approval`,
      body: permissionBody(input),
    });
    return true;
  } catch {
    return false;
  }
}
