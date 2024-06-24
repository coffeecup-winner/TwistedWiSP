extends GraphNode
class_name FlowGraphNode

var flow_node: TwistedWispFlowNode


func _ready():
	flow_node.connect("property_value_changed", _on_property_value_changed)


func _on_property_value_changed(prop_name, new_value):
	match prop_name:
		"x": position_offset.x = new_value
		"y": position_offset.y = new_value
		"w": size.x = new_value
		"y": size.y = new_value
