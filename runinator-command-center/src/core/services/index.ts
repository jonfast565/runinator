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
import { createPermissionsService } from "./permissions";
import { createWorkflowServices } from "./workflows";
import { createWdlLanguageService } from "./wdl-language";
import { createExpressionService } from "./expression";
import { createAuditLogService } from "./audit-log";
import { createDeadLettersService } from "./dead-letters";
import { createReplicaSamplesService } from "./replica-samples";
import { createDevPackService } from "./dev-pack";
import { createOrgAdminService } from "./org-admin";
import { createOrgResourcesService } from "./org-resources";
import { createWorkflowSharingService } from "./workflow-sharing";
import { createWorkflowRunExtrasService } from "./workflow-run-extras";
import { createNodePoolsService } from "./node-pools";
import { createSupervisorService } from "./supervisor";

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
export const wdlLanguageService = createWdlLanguageService(appService);
export const expressionService = createExpressionService(appService);
export const auditLogService = createAuditLogService(appService);
export const deadLettersService = createDeadLettersService(appService);
export const replicaSamplesService = createReplicaSamplesService(appService);
export const devPackService = createDevPackService(appService);
export const orgAdminService = createOrgAdminService(appService);
export const orgResourcesService = createOrgResourcesService(appService);
export const workflowSharingService = createWorkflowSharingService(appService);
export const workflowRunExtrasService = createWorkflowRunExtrasService(appService);
export const nodePoolsService = createNodePoolsService(appService);
export const supervisorService = createSupervisorService();

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
export type { PermissionsService } from "./permissions";
export type { WorkflowServices } from "./workflows";
export type { WorkflowServiceDeps } from "./workflows/host";
export type { WorkflowRunExtrasService } from "./workflow-run-extras";
export type { NodePoolsService, NodeBackendInfo, ProvisionedGroup, ScaleNodesRequest } from "./node-pools";
export type { SupervisorService, SupervisorStatus } from "./supervisor";
export type { WdlLanguageService } from "./wdl-language";
export type { ExpressionService } from "./expression";
export type { AuditLogService } from "./audit-log";
export type { DeadLettersService } from "./dead-letters";
export type { ReplicaSamplesService, ReplicaSample, ReplicaSampleSeries } from "./replica-samples";
export type { DevPackService } from "./dev-pack";
export type { OrgAdminService, OrgMembership, OrgRole, Team, User } from "./org-admin";
export type { OrgResourcesService, OrgQuota, OrgResourceGroup, OrgUsage, RateCard } from "./org-resources";
export type { WorkflowSharingService } from "./workflow-sharing";
