import '../../core/services/workflows_service.dart';
import '../../core/workflow/graph_model.dart';

class WorkflowGraphBridge {
  WorkflowGraphBridge(this._workflows);

  final WorkflowsNotifier _workflows;

  List<GraphNodeModel> get nodes => _workflows.host.buildDraftGraphNodes();

  List<GraphEdgeModel> get edges => _workflows.host.buildDraftGraphEdges();

  void onNodeClick(String nodeId, {bool shiftKey = false}) {
    _workflows.editor.dismissStepEditorForCanvasEdit();
    _workflows.host.state.selectedGraphEdgeId = '';
    _workflows.host.state.inlineEditNodeId = '';
    _workflows.editor.populateStepEditor(nodeId);
    _workflows.host.notify();
  }

  void onNodeDoubleClick(String nodeId) {
    _workflows.host.state.selectedGraphEdgeId = '';
    _workflows.editor.populateStepEditor(nodeId);
    _workflows.host.state.inlineEditNodeId = nodeId;
    _workflows.host.notify();
  }

  void onNodeDragEnd(String nodeId, GraphPosition position) {
    _workflows.editor.dismissStepEditorForCanvasEdit();
    _workflows.editor.setGraphNodePosition(nodeId, position);
    _workflows.editor.syncWorkflowDraftToJson();
  }

  void onEdgeClick(String edgeId) {
    _workflows.editor.selectGraphEdge(edgeId);
    _workflows.host.notify();
  }

  void onEdgeRemove(String edgeId) {
    _workflows.editor.removeWorkflowEdgeById(edgeId);
    _workflows.host.notify();
  }

  void clearSelection() {
    _workflows.editor.clearWorkflowGraphSelection();
    _workflows.host.notify();
  }
}
