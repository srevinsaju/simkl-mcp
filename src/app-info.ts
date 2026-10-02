import packageJson from '../package.json' with { type: 'json' };

export const APP_NAME = packageJson.name;
export const APP_VERSION = packageJson.version;
export const USER_AGENT = `${APP_NAME}/${APP_VERSION}`;
