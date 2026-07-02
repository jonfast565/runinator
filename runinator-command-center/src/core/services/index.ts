import { createAppService } from "./app";
import { createAuthService } from "./auth";
import { createGatesService } from "./gates";
import { createResourcesService } from "./resources";

export const appService = createAppService();
export const authService = createAuthService();
export const resourcesService = createResourcesService(appService);
export const gatesService = createGatesService(appService);

export type { AppService } from "./app";
export type { AuthService } from "./auth";
export type { ResourcesService } from "./resources";
export type { GatesService } from "./gates";
