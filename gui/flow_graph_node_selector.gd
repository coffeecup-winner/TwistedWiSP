extends LineEdit

var FlowGraphNode = preload("res://flow_graph_node.tscn")


func _on_text_submitted(new_text):
	var node = FlowGraphNode.instantiate()
	node.title = new_text
	node.position_offset = self.position
	node.size = Vector2(80, 80)
	get_parent().add_flow_node(node)
	# Force the focus exit handler to fire
	get_parent().grab_focus()


func _on_focus_exited():
	get_parent().remove_child.call_deferred(self)
