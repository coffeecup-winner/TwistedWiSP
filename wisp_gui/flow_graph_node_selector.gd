extends LineEdit

var FlowGraphNode = preload("res://flow_graph_node.tscn")


func _on_text_submitted(new_text):
	var node = FlowGraphNode.instantiate()
	node.title = new_text
	node.position_offset = self.position
	node.size = Vector2(80, 80)
	
	var metadata = TwistedWisp.function_get_metadata(new_text)
	var rows_count = max(metadata.num_inlets, metadata.num_outlets)
	
	for i in range(0, rows_count):
		node.add_child(Label.new())
	
	for i in range(0, metadata.num_inlets):
		node.set_slot_enabled_left(i, true)
	
	for i in range(0, metadata.num_outlets):
		node.set_slot_enabled_right(i, true)
	
	get_parent().add_flow_node(node)
	# Force the focus exit handler to fire
	get_parent().grab_focus()


func _on_focus_exited():
	get_parent().remove_child.call_deferred(self)
