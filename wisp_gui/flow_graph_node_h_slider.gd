extends FlowGraphNode


func _ready():
	$HSlider.value = flow_node.get_data_value()


func _on_h_slider_value_changed(value):
	flow_node.set_data_value(value)
