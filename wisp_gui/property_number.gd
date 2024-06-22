extends HBoxContainer
class_name PropertyNumber


var flow_node: TwistedWispFlowNode
var property_name: String


func _on_control_value_changed(value: float):
	if flow_node:
		flow_node.set_property_number(property_name, value)


func initialize(node: TwistedWispFlowNode, prop: TwistedWispFlowNodeProperty):
	flow_node = node
	property_name = prop.name
	$Label.text = prop.display_name
	$Control.min_value = prop.min_value
	$Control.max_value = prop.max_value
	$Control.value = flow_node.get_property_number(property_name)
