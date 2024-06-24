extends HBoxContainer
class_name PropertyNumber


var flow_node: TwistedWispFlowNode
var property_name: String
var value_type: String


func _on_control_value_changed(value: float):
	if flow_node:
		if value_type == "float":
			flow_node.set_property_value(property_name, value)
		else:
			flow_node.set_property_value(property_name, int(value))


func initialize(node: TwistedWispFlowNode, prop: TwistedWispFlowNodePropertyData):
	flow_node = node
	property_name = prop.name
	value_type = prop.value_type
	$Label.text = prop.display_name
	$Control.min_value = prop.min_value
	$Control.max_value = prop.max_value
	$Control.value = flow_node.get_property_value(property_name)
