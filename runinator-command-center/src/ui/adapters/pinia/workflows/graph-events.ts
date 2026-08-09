import type {
  Connection,
  EdgeChange,
  EdgeMouseEvent,
  EdgeUpdateEvent,
  NodeChange,
  NodeDragEvent,
  NodeMouseEvent,
} from "@vue-flow/core";
import type { WorkflowServices } from "../../../../core/services";
import { optionIdForSourceHandle } from "../../../../core/workflow";

/** vue-flow event adapter for workflow graph editing. */
export function createWorkflowGraphHandlers(services: WorkflowServices) {
  function onGraphNodeClick(event: NodeMouseEvent) {
    const nodeId = event.node.id;

    if (!nodeId) {
      return;
    }

    services.editor.dismissStepEditorForCanvasEdit();
    services.setState((current) => ({
      ...current,
      selectedGraphEdgeId: "",
      inlineEditNodeId: "",
    }));
    services.editor.populateStepEditor(nodeId);
  }

  function onGraphNodeDoubleClick(event: NodeMouseEvent) {
    const nodeId = event.node.id;

    if (!nodeId) {
      return;
    }

    services.setState((current) => ({ ...current, selectedGraphEdgeId: "" }));
    services.editor.populateStepEditor(nodeId);
    services.setState((current) => ({ ...current, inlineEditNodeId: nodeId }));
  }

  function onGraphNodeDragStop(event: NodeDragEvent) {
    const node = event.node;

    if (!node.id) {
      return;
    }

    services.editor.dismissStepEditorForCanvasEdit();
    services.editor.setGraphNodePosition(node.id, node.position);
    services.editor.syncWorkflowDraftToJson();
  }

  function onGraphNodesChange(changes: NodeChange[]) {
    let changed = false;

    for (const change of changes) {
      if (change.type !== "position" || !change.id || change.dragging) {
        continue;
      }

      services.editor.setGraphNodePosition(change.id, change.position);
      changed = true;
    }

    if (changed) {
      services.editor.syncWorkflowDraftToJson();
    }
  }

  function onGraphConnect(connection: Connection) {
    const source = connection.source;
    const handleOptionId = optionIdForSourceHandle(connection.sourceHandle) ?? undefined;
    const options = services.editor.workflowEdgeOptions(source);

    if (!source || options.length === 0) {
      return;
    }

    const optionId =
      handleOptionId && options.some((option) => option.id === handleOptionId)
        ? handleOptionId
        : options.length === 1
          ? options[0].id
          : "";

    if (optionId) {
      services.editor.applyGraphEdgeSemantic(connection, optionId);
    }
  }

  function onGraphEdgeClick(event: EdgeMouseEvent) {
    if (event.edge.id) {
      services.editor.selectGraphEdge(event.edge.id);
    }
  }

  function onGraphEdgeUpdate(event: EdgeUpdateEvent) {
    const { edge, connection } = event;

    if (!connection.source || !connection.target) {
      return;
    }

    if (
      services.editor.applyGraphEdgeSemantic(connection, edge.id, edge.id) &&
      services.getState().selectedStepId === edge.source
    ) {
      services.editor.populateStepEditor(edge.source);
    }

    services.setState((current) => ({ ...current, selectedGraphEdgeId: "" }));
  }

  function onGraphEdgesChange(changes: EdgeChange[]) {
    for (const change of changes) {
      if (change.type === "remove") {
        services.editor.removeWorkflowEdgeById(change.id);
      }
    }
  }

  return {
    onGraphNodeClick,
    onGraphNodeDoubleClick,
    onGraphNodeDragStop,
    onGraphNodesChange,
    onGraphConnect,
    onGraphEdgeClick,
    onGraphEdgeUpdate,
    onGraphEdgesChange,
  };
}
