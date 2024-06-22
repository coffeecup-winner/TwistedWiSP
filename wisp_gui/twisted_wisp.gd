extends Control

const FlowGraphView = preload("res://flow_graph_view.tscn")

var wisp: TwistedWisp = null

func _ready():
	var config = FileAccess.open("res://wisp.toml", FileAccess.READ)
	var config_text = config.get_as_text()
	wisp = TwistedWisp.create(config_text)
	var graph = FlowGraphView.instantiate()
	graph.connect("node_selected", _on_flow_graph_node_selected)
	graph.connect("node_deselected", _on_flow_graph_node_deselected)
	graph.wisp = wisp
	self.add_child(graph)
	graph.grab_focus()


func _on_flow_graph_node_selected(node):
	if node is FlowGraphNode:
		$PropertyInspector.flow_node = node.flow_node


func _on_flow_graph_node_deselected(node):
	if node is FlowGraphNode:
		if $PropertyInspector.flow_node == node.flow_node:
			$PropertyInspector.flow_node = null
