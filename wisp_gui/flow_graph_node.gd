extends GraphNode
class_name FlowGraphNode

var flow_node: TwistedWispFlowNode


func _ready():
	flow_node.connect("coordinates_changed", _on_coordinates_changed)


func _on_coordinates_changed(x, y, w, h):
	position_offset.x = x
	position_offset.y = y
	size.x = w
	size.y = h
