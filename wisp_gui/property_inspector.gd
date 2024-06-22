extends VBoxContainer
class_name PropertyInspector


const PROPERTY_NUMBER = preload("res://property_number.tscn")


var flow_node: TwistedWispFlowNode:
	get = _get_flow_node,
	set = _set_flow_node


func _get_flow_node() -> TwistedWispFlowNode:
	return flow_node


func _set_flow_node(value):
	flow_node = value
	if flow_node == null:
		for node in get_children():
			remove_child(node)
			node.queue_free()
	else:
		for prop in flow_node.get_properties():
			var node = PROPERTY_NUMBER.instantiate()
			node.initialize(flow_node, prop)
			add_child(node)
