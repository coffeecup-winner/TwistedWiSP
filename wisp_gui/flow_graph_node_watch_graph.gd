extends "res://flow_graph_node.gd"


var graph_size = Vector2(140, 50)
var data = []


func process_watch_updates(values):
	data.append_array(values)
	var length = graph_size.x
	if len(data) > length:
		data = data.slice(-length)

	while $Graph/GraphLine.get_point_count() > length:
		$Graph/GraphLine.remove_point($Graph/GraphLine.get_point_count() - 1)
	while $Graph/GraphLine.get_point_count() < length:
		$Graph/GraphLine.add_point(Vector2.ZERO)
	
	var center_y = graph_size.y / 2
	for i in range(0, len(data)):
		var v = data[i]
		$Graph/GraphLine.set_point_position(i, Vector2(i, center_y - v * center_y))


func _on_resized():
	graph_size = size - Vector2(40, 60)
	$Graph.polygon = PackedVector2Array([
		Vector2(0, 0),
		Vector2(0, graph_size.y),
		Vector2(graph_size.x, graph_size.y),
		Vector2(graph_size.x, 0),
	])
	var center_y = graph_size.y / 2
	$Graph/CenterLine.points = PackedVector2Array([
		Vector2(0, center_y),
		Vector2(graph_size.x, center_y),
	])
