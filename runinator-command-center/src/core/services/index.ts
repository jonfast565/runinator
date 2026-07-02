import { createAppService } from "./app";
import { createAuthService } from "./auth";
import { createGatesService } from "./gates";
import { createResourcesService } from "./resources";
import { createArtifactsService } from "./artifacts";
import { createNotificationsService } from "./notifications";
import { createSecretsService } from "./secrets";
import { createProvidersService } from "./providers";
import { createOrgsService } from "./orgs";
import { createAdminSettingsService } from "./admin-settings";
import { createDisplayPreferencesService } from "./display-preferences";
import { createLocalWorkerService } from "./local-worker";
import { createPermissionsService } from "./permissions";
import { createWorkflowServices } from "./workflows";

export const appService = createAppService();
export const authService = createAuthService();
export const resourcesService = createResourcesService(appService);
export const gatesService = createGatesService(appService);
export const artifactsService = createArtifactsService(appService);
export const notificationsService = createNotificationsService(appService);
export const secretsService = createSecretsService(appService);
export const providersService = createProvidersService();
export const orgsService = createOrgsService(appService, authService);
export const adminSettingsService = createAdminSettingsService(appService);
export const displayPreferencesService = createDisplayPreferencesService();
export const localWorkerService = createLocalWorkerService();
export const permissionsService = createPermissionsService(appService);

export const workflowServices = createWorkflowServices({
  app: appService,
  getProviders: () => providersService.getState().providers,
  refreshResources: () => {
    void resourcesService.refreshResources();
  },
});
export const workflowCatalogService = workflowServices.catalog;
export const workflowEditorService = workflowServices.editor;
export const workflowRunService = workflowServices.runs;

export type { AppService } from "./app";
export type { AuthService } from "./auth";
export type { ResourcesService } from "./resources";
export type { GatesService } from "./gates";
export type { ArtifactsService } from "./artifacts";
export type { NotificationsService } from "./notifications";
export type { SecretsService } from "./secrets";
export type { ProvidersService } from "./providers";
export type { OrgsService } from "./orgs";
export type { AdminSettingsService } from "./admin-settings";
export type { DisplayPreferencesService } from "./display-preferences";
export type { LocalWorkerService } from "./local-worker";
export type { PermissionsService } from "./permissions";
export type { WorkflowServices } from "./workflows";
export type { WorkflowServiceDeps } from "./workflows/host";
