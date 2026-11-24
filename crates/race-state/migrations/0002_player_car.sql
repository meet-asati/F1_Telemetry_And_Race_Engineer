-- Which car the player drove, so the debrief knows whose race it is.
ALTER TABLE sessions ADD COLUMN player_car INTEGER;
