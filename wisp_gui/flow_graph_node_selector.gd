extends LineEdit

func _on_text_submitted(new_text):
	get_parent().add_flow_node(new_text, null, self.position)
	# Force the focus exit handler to fire
	get_parent().grab_focus()


func _on_focus_exited():
	get_parent().remove_child.call_deferred(self)
	self.queue_free()
